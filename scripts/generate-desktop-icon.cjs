// Run with Node and sharp installed. Desktop uses the existing vector mark;
// mobile assets retain their platform-specific padding.
const path = require('node:path');
const sharp = require('sharp');
const root = path.resolve(__dirname, '..', 'app', 'assets', 'icons');
sharp(path.join(root, 'stashi-wallet-logo.svg'), { density: 96 })
  .resize(1024, 1024)
  .trim()
  .resize(992, 992, { fit: 'contain', background: '#00000000' })
  .extend({ top: 16, bottom: 16, left: 16, right: 16, background: '#00000000' })
  .png()
  .toFile(path.join(root, 'stashi-wallet-desktop-icon.png'))
  .catch(error => { console.error(error); process.exitCode = 1; });
