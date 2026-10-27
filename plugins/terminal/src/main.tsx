import '@sodeck/ui/styles.css';
import { createRoot } from 'react-dom/client';
import { initializeTheme } from '@sodeck/sdk';
import { TerminalView } from './terminal-view';

import './terminal.css';
void initializeTheme().then(theme=>createRoot(document.getElementById('root')!).render(<TerminalView theme={theme}/>)).catch(e=>{document.getElementById('root')!.textContent=String(e);});
