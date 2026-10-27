import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({base:'./',plugins:[react()],define:{__FILES_VERSION__:JSON.stringify(process.env.FILES_PLUGIN_VERSION??'0.2.0')},build:{outDir:'dist/ui',emptyOutDir:true,target:'es2022'}});
