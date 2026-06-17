import { StrictMode } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/index.css';

// StrictMode is safe: TerminalSession defers terminal creation to the next
// animation frame so the first StrictMode mount can dispose before heavy
// renderer work is scheduled.
ReactDOM.createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
