import '@sodeck/ui/styles.css';
import {createRoot} from 'react-dom/client';
import {initializeTheme,dialogContext} from '@sodeck/sdk';
import {EditorDialog} from './editor-dialog';
import {View} from './view';
import './network.css';
void initializeTheme().then(()=>createRoot(document.getElementById('root')!).render(dialogContext()?<EditorDialog/>:<View/>)).catch(error=>{document.getElementById('root')!.textContent=String(error);});
