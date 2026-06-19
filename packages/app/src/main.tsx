import { StrictMode } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import PwaApp from './PwaApp';
import './styles/index.css';

const isMobileOrPwa =
  new URLSearchParams(window.location.search).has('pwa') ||
  window.matchMedia('(display-mode: standalone)').matches ||
  /Android|iPhone|iPad/i.test(navigator.userAgent);

// StrictMode is safe: TerminalSession defers terminal creation to the next
// animation frame so the first StrictMode mount can dispose before heavy
// renderer work is scheduled.
const Root = isMobileOrPwa ? PwaApp : App;
ReactDOM.createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
