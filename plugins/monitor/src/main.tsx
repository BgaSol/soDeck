import '@sodeck/ui/styles.css';
import './monitor.css';
import {createRoot} from 'react-dom/client';
import {initializeTheme} from '@sodeck/sdk';
import {MonitorView} from './view';
void initializeTheme().then(()=>createRoot(document.getElementById('root')!).render(<MonitorView/>)).catch(error=>{document.getElementById('root')!.textContent=String(error);});
