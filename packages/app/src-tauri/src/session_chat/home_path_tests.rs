use super::home_to_tilde;

#[test]
fn the_four_forms_of_a_user_folder_become_a_tilde() {
    // `/mnt/<letter>/Users/<name>` (WSL's view of a Windows user folder).
    assert_eq!(home_to_tilde("/mnt/c/Users/you", None), "~");
    assert_eq!(home_to_tilde("/mnt/c/Users/you/proj/a.rs", None), "~/proj/a.rs");
    // `\\wsl.localhost\<distro>\home\<name>` and `\\wsl$\<distro>\home\<name>`.
    assert_eq!(home_to_tilde(r"\\wsl.localhost\Ubuntu\home\you", None), "~");
    assert_eq!(home_to_tilde(r"\\wsl$\Ubuntu-Test\home\you\p", None), r"~\p");
    // Either slash, any case.
    assert_eq!(home_to_tilde("//WSL.localhost/ubuntu/Home/you/p", None), "~/p");
    assert_eq!(home_to_tilde(r"\\WSL$\ubuntu\HOME\you\p", None), r"~\p");
    // The forms that were already handled keep their result.
    assert_eq!(home_to_tilde(r"C:\Users\you\x", None), r"~\x");
    assert_eq!(home_to_tilde("/home/you/p", None), "~/p");
}

#[test]
fn a_user_folder_with_a_space_becomes_a_tilde_too() {
    assert_eq!(home_to_tilde("/mnt/c/Users/Default User/proj/a.rs", None), "~/proj/a.rs");
    assert_eq!(home_to_tilde(r"\\wsl.localhost\Ubuntu\home\Default User\proj", None), r"~\proj");
    assert_eq!(home_to_tilde("open /mnt/c/Users/Default User/x now", None), "open ~/x now");
}

#[test]
fn a_mnt_users_folder_is_masked_like_a_windows_users_folder() {
    assert_eq!(home_to_tilde("/mnt/c/Users/Public", None), "~");
    assert_eq!(home_to_tilde(r"C:\Users\Public", None), "~");
    // Never cut into `/mnt/c~`: the drive part stays out of the replacement.
    assert_eq!(home_to_tilde("/mnt/c/Users/you/x", None), "~/x");
    assert!(!home_to_tilde("/mnt/c/Users/you/x", None).contains("/mnt/c~"));
}

#[test]
fn look_alike_folders_are_left_alone() {
    assert_eq!(home_to_tilde("/homework/x", None), "/homework/x");
    assert_eq!(home_to_tilde(r"C:\home2\x", None), r"C:\home2\x");
    assert_eq!(home_to_tilde(r"\\wsl.localhost\Ubuntu\homework\x", None), r"\\wsl.localhost\Ubuntu\homework\x");
    assert_eq!(home_to_tilde(r"\\wsl.localhost\Ubuntu\home2\x", None), r"\\wsl.localhost\Ubuntu\home2\x");
    assert_eq!(home_to_tilde(r"\\wsl.localhost\Debian\x", None), r"\\wsl.localhost\Debian\x");
    assert_eq!(home_to_tilde("/mnt/cc/Users/x", None), "/mnt/cc/Users/x");
    assert_eq!(home_to_tilde("/mnt/c/Users", None), "/mnt/c/Users");
}
