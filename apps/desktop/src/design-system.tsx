import {createRoot} from 'react-dom/client';
import '@sodeck/ui/styles.css';
import {Catalog} from '../../../tests/ui/design-system/catalog';
createRoot(document.getElementById('root')!).render(<Catalog/>);
