import {defineConfig} from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({base:'./',plugins:[react()],define:{__NETWORK_VERSION__:JSON.stringify(process.env.NETWORK_PLUGIN_VERSION??'0.1.0')},build:{outDir:'dist/ui',emptyOutDir:true,target:'es2022'}});
