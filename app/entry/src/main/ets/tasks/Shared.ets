import { image } from '@kit.ImageKit';
import { drawing } from '@kit.ArkGraphics2D';
import { fileIo as fs } from '@kit.CoreFileKit';

/**
 * TaskPool 任务的公共工具。
 *
 * 注意：`@Concurrent` 函数只能引用 import 进来的符号或局部变量，
 * 所以这些辅助函数必须放在独立模块里被 import，不能写成同文件的顶层函数。
 */
export function writeBytes(path: string, bytes: ArrayBuffer): void {
  const idx = path.lastIndexOf('/');
  if (idx > 0) {
    const dir = path.substring(0, idx);
    try {
      if (!fs.accessSync(dir)) {
        fs.mkdirSync(dir, true);
      }
    } catch (err) {
      fs.mkdirSync(dir, true);
    }
  }
  const file = fs.openSync(path, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
  try {
    fs.writeSync(file.fd, bytes);
  } finally {
    fs.closeSync(file);
  }
}

/**
 * 阴影补偿：估算全图平均亮度，按强度做 gamma 增益。
 * 比完整光照图便宜，但足以把手机拍出的灰底拉回纸白。
 */
export async function compensateShadow(pm: image.PixelMap, w: number, h: number,
  enhanced: number): Promise<void> {
  const buffer = new ArrayBuffer(w * h * 4);
  await pm.readPixelsToBuffer(buffer);
  const pixels = new Uint8Array(buffer);

  let sum = 0;
  let count = 0;
  for (let i = 0; i + 2 < pixels.length; i += 28) {
    sum += (pixels[i] * 299 + pixels[i + 1] * 587 + pixels[i + 2] * 114) / 1000;
    count++;
  }
  if (count === 0) {
    return;
  }
  const mean = Math.max(1, sum / count);
  const strength = enhanced / 100 * 0.9;
  // 只提亮：白纸保持白，暗部（阴影）被抬起
  let gain = Math.pow(245 / mean, strength);
  gain = Math.min(2.4, Math.max(1.0, gain));

  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const r = pixels[i] * gain;
    const g = pixels[i + 1] * gain;
    const b = pixels[i + 2] * gain;
    pixels[i] = r > 255 ? 255 : r;
    pixels[i + 1] = g > 255 ? 255 : g;
    pixels[i + 2] = b > 255 ? 255 : b;
  }
  await pm.writeBufferToPixels(buffer);
}

/**
 * 把归一化笔迹画成纸白色：圆点补笔头，线段连笔身。
 *
 * 放在这里而不是 WarpTask 里，是因为 `@Concurrent` 函数只能引用 import 进来的符号。
 */
export function eraseStrokes(canvas: drawing.Canvas, w: number, h: number,
  erase: number[]): void {
  const brush = new drawing.Brush();
  brush.setColor(0xFFFFFFFF);
  brush.setAntiAlias(true);
  canvas.attachBrush(brush);
  let i = 0;
  while (i + 1 < erase.length) {
    const x = erase[i];
    const y = erase[i + 1];
    const r = i + 2 < erase.length ? erase[i + 2] : 0.02;
    i += 3;
    if (x < 0 || r <= 0) {
      continue;
    }
    canvas.drawCircle(x * w, y * h, Math.max(0.5, r * w));
  }
  canvas.detachBrush();

  const pen = new drawing.Pen();
  pen.setColor(0xFFFFFFFF);
  pen.setAntiAlias(true);
  pen.setCapStyle(drawing.CapStyle.ROUND_CAP);
  pen.setJoinStyle(drawing.JoinStyle.ROUND_JOIN);
  canvas.attachPen(pen);
  let lastX = -1;
  let lastY = 0;
  let width = -1;
  let j = 0;
  while (j + 1 < erase.length) {
    const x = erase[j];
    const y = erase[j + 1];
    const r = j + 2 < erase.length ? erase[j + 2] : 0.02;
    j += 3;
    if (x < 0) {
      lastX = -1;
      continue;
    }
    const penWidth = Math.max(1, r * w * 2);
    if (penWidth !== width) {
      width = penWidth;
      pen.setStrokeWidth(penWidth);
    }
    if (lastX >= 0) {
      canvas.drawLine(lastX * w, lastY * h, x * w, y * h);
    }
    lastX = x;
    lastY = y;
  }
  canvas.detachPen();
}
