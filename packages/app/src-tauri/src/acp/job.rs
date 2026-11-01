//! Keeps an adapter and everything it starts from outliving it.
//!
//! * **Windows**: the adapter is put in a Job Object with `KILL_ON_JOB_CLOSE`. When the app exits or
//!   crashes (the last handle to the job closes), Windows kills every process in the job. This is the
//!   Windows counterpart of Linux's "die with parent": the Node adapter, the agent CLI under it and that
//!   CLI's MCP servers (Codex starts about 25 processes) all go. Terminating the job is also how a
//!   session is stopped. The adapter is assigned a moment after it starts; the gap is far shorter than
//!   the adapter's own start-up, so it has started nothing yet. Jobs nest on Windows 8 and later.
//! * **Elsewhere** (and on Windows when the job cannot be made): the adapter leads its own process group
//!   and a stop kills the tree below its pid through `pty::proc_tree`, by pid and never by image name.
//!   There is no "die with the app" there; a stale-adapter sweep at start (A3) covers a crashed app.

use std::process::{Child, Command};

/// How a running adapter is contained.
#[derive(Debug)]
pub struct Containment {
    pid: u32,
    #[cfg(windows)]
    job: Option<win::Job>,
}

/// Which mechanism holds the adapter (for diagnostics and tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mechanism {
    JobObject,
    ProcessTree,
}

/// Before the child starts: put it in its own process group where that is how it will be contained.
pub fn prepare(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

impl Containment {
    /// Contains `child`, which was just spawned.
    pub fn attach(child: &Child) -> Containment {
        let pid = child.id();
        #[cfg(windows)]
        {
            let job = match win::Job::new() {
                Ok(job) => match job.assign(child) {
                    Ok(()) => Some(job),
                    Err(e) => {
                        tracing::warn!(pid, error = %e, "acp: could not put the adapter in a job object; falling back to killing its tree");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(pid, error = %e, "acp: could not create a job object; falling back to killing the tree");
                    None
                }
            };
            Containment { pid, job }
        }
        #[cfg(not(windows))]
        {
            Containment { pid }
        }
    }

    pub fn mechanism(&self) -> Mechanism {
        #[cfg(windows)]
        {
            if self.job.is_some() {
                return Mechanism::JobObject;
            }
        }
        Mechanism::ProcessTree
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Kills the adapter and everything below it. Idempotent.
    pub fn terminate(&self) {
        #[cfg(windows)]
        {
            if let Some(job) = &self.job {
                job.terminate();
                return;
            }
        }
        crate::pty::proc_tree::kill_process_tree(self.pid);
    }

    /// Processes still alive in the job (the adapter included); `None` when there is no job to ask.
    pub fn live_processes(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            self.job.as_ref().and_then(|job| job.active_processes())
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    #[derive(Debug)]
    pub struct Job(HANDLE);

    // SAFETY: a job handle is a kernel object handle; the calls used on it are thread-safe.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> io::Result<Job> {
            // SAFETY: plain Win32 calls with valid pointers; the handle is owned by the returned Job.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle == 0 {
                    return Err(io::Error::last_os_error());
                }
                let job = Job(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(job)
            }
        }

        pub fn assign(&self, child: &Child) -> io::Result<()> {
            // SAFETY: both handles are valid for the duration of the call.
            let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub fn terminate(&self) {
            // SAFETY: valid job handle. Failure means the job is already empty.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }

        pub fn active_processes(&self) -> Option<u32> {
            // SAFETY: the out structure is zeroed, correctly sized and lives across the call.
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                let ok = QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                );
                if ok == 0 {
                    None
                } else {
                    Some(info.ActiveProcesses)
                }
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the last handle kills whatever is still in the job (KILL_ON_JOB_CLOSE).
            // SAFETY: the handle is owned and closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
