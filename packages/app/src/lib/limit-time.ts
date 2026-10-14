// "14:05" for a moment today, "Oct 18 17:00" for another day: how a usage-limit reset time reads in the app.
// The same shape as the backend's card text (`src-tauri/src/handoff/reset.rs::when_text`).

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

const two = (n: number): string => String(n).padStart(2, '0');

export function resetTimeText(ms: number, now: number = Date.now()): string {
  const at = new Date(ms);
  const today = new Date(now);
  const clock = `${two(at.getHours())}:${two(at.getMinutes())}`;
  const sameDay = at.getFullYear() === today.getFullYear() && at.getMonth() === today.getMonth() && at.getDate() === today.getDate();
  return sameDay ? clock : `${MONTHS[at.getMonth()]} ${at.getDate()} ${clock}`;
}
