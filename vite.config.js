import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteStaticCopy } from 'vite-plugin-static-copy';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [
    svelte(),
    viteStaticCopy({
      targets: [
        {
          src: 'static/bg.jpg',
          dest: '../build/static',
        },
        {
          src: 'static/lang',
          dest: '../build/static',
        },
      ],
    }),
  ],

  root: 'src',
  clearScreen: false,
  build: {
    assetsInlineLimit: 0,
    target: process.env.TAURI_PLATFORM == 'windows' ? 'edge108' : ['es2021', 'safari14'],
    outDir: "../build",
  },
  envPrefix: ['VITE_', 'TAURI_PLATFORM', 'TAURI_ARCH', 'TAURI_FAMILY', 'TAURI_PLATFORM_VERSION', 'TAURI_PLATFORM_TYPE', 'TAURI_DEBUG'],
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
        protocol: 'ws',
        host,
        port: 1421,
      }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**', '../build'],
    },
  },
});
