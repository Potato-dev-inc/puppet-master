import { useEffect } from 'react';
import '../../styles/askbar.css';
import '../../styles/dot-spinner.css';
import { AskBar } from './AskBar';

/** Root of the Ask bar window (its own Tauri window, separate from the cockpit). */
export default function AskBarApp(): JSX.Element {
  useEffect(() => {
    document.documentElement.classList.add('askbar-root');
    return () => document.documentElement.classList.remove('askbar-root');
  }, []);
  return <AskBar />;
}
