/** @type {import("vite").UserConfig} */
export default {
  root: 'dist',
  publicDir: false,
  server: {
    fs: {
      allow: ['..'],
    },
  },
  build: {
    outDir: '../dist-build',
    cssCodeSplit: true,
    lib: {
      entry: '../latex.css',
      formats: ['es'],
      fileName: 'latex',
    },
    rollupOptions: {
      output: {
        assetFileNames: 'latex.css',
      },
    },
  },
};
