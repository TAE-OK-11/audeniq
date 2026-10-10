import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import '../../../studio/app/src/styles/tokens.css';
import './style.css';
import '../../../studio/app/src/styles/controls.css';
import './studio-align.css';

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>);

