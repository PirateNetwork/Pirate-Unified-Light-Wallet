const {spawnSync} = require('node:child_process');
const {Buffer} = require('node:buffer');

const imageSize = require('image-size');

const MALFORMED_IMAGES = {
  HEIF: [
    0, 0, 0, 16, 102, 116, 121, 112, 97, 118, 105, 102, 0, 0, 0, 0, 0, 0, 0, 36,
    109, 101, 116, 97, 0, 0, 0, 0, 0, 0, 0, 8, 105, 112, 114, 112, 0, 0, 0, 20,
    105, 112, 99, 111, 0, 0, 0, 0, 105, 115, 112, 101, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
  ],
  ICNS: [105, 99, 110, 115, 0, 0, 0, 16, 105, 115, 51, 50, 0, 0, 0, 0],
  JXL: [0, 0, 0, 0, 74, 88, 76, 32],
};

it('preserves the callable image-size API expected by Metro', () => {
  const pngHeader = Buffer.from([
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0,
    0, 0, 3,
  ]);

  expect(typeof imageSize).toBe('function');
  expect(imageSize(pngHeader)).toEqual({height: 3, width: 2, type: 'png'});
});

it('rejects malformed JXL, HEIF, and ICNS structures without hanging', () => {
  const parserScript = `
    const imageSize = require('image-size');
    const payloads = ${JSON.stringify(Object.values(MALFORMED_IMAGES))};

    for (const bytes of payloads) {
      try {
        imageSize(Buffer.from(bytes));
      } catch {}
    }
  `;
  const result = spawnSync(process.execPath, ['-e', parserScript], {
    cwd: process.cwd(),
    timeout: 5000,
  });

  expect(result.error).toBeUndefined();
  expect(result.status).toBe(0);
});
