import { image } from '@kit.ImageKit';
import { drawing, common2D } from '@kit.ArkGraphics2D';
import { writeBytes, compensateShadow, eraseStrokes } from './Shared';

export interface WarpResult {
  ok: boolean;
  width: number;
  height: number;
  message: string;
}

/**
 * 透视矫正 + 去阴影补偿 + 擦除 + 裁剪，全程 Skia，无逐像素插值循环。
 *
 * `quad` 是归一化四角（左上、右上、右下、左下），`enhanced` 0~100。
 * `crop` 是归一化裁剪框 `[x1, y1, x2, y2]`，`applyCrop` 为 false 时保留整幅
 * （编辑页需要未裁剪的图来摆放裁剪框）。
 * `erase` 是归一化笔迹点 `[x, y, r, ...]`，`x < 0` 表示抬笔分隔。
 * `maxEdge` 控制输出长边：预览 640，导出 2000。
 */
@Concurrent
export async function warpAndSave(srcPath: string, outPath: string, quad: number[],
  enhanced: number, maxEdge: number, crop: number[], erase: number[],
  applyCrop: boolean): Promise<WarpResult> {
  let srcPm: image.PixelMap | undefined = undefined;
  let fullPm: image.PixelMap | undefined = undefined;
  let cropPm: image.PixelMap | undefined = undefined;
  let source: image.ImageSource | undefined = undefined;
  try {
    source = image.createImageSource(srcPath);
    const info = await source.getImageInfo();
    const srcW = info.size.width;
    const srcH = info.size.height;
    if (srcW <= 0 || srcH <= 0) {
      const bad: WarpResult = { ok: false, width: 0, height: 0, message: '原图尺寸异常' };
      return bad;
    }

    const scale = Math.max(srcW, srcH) > maxEdge ? maxEdge / Math.max(srcW, srcH) : 1;
    const decW = Math.max(2, Math.round(srcW * scale));
    const decH = Math.max(2, Math.round(srcH * scale));
    const decodeOpt: image.DecodingOptions = {
      desiredSize: { width: decW, height: decH }
    };
    srcPm = await source.createPixelMap(decodeOpt);

    const q = quad.length === 8 ? quad : [0, 0, 1, 0, 1, 1, 0, 1];
    const px: number[] = [];
    for (let i = 0; i < 8; i += 2) {
      px.push(q[i] * decW);
      px.push(q[i + 1] * decH);
    }

    let minX = px[0];
    let maxX = px[0];
    let minY = px[1];
    let maxY = px[1];
    for (let i = 2; i < 8; i += 2) {
      minX = Math.min(minX, px[i]);
      maxX = Math.max(maxX, px[i]);
      minY = Math.min(minY, px[i + 1]);
      maxY = Math.max(maxY, px[i + 1]);
    }
    const outW = Math.max(8, Math.round(maxX - minX));
    const outH = Math.max(8, Math.round(maxY - minY));

    fullPm = image.createPixelMapSync({
      size: { width: outW, height: outH },
      pixelFormat: image.PixelMapFormat.RGBA_8888,
      alphaType: image.AlphaType.OPAQUE,
      editable: true
    });

    const canvas = new drawing.Canvas(fullPm);
    const brush = new drawing.Brush();
    brush.setColor(0xFFFFFFFF);
    canvas.attachBrush(brush);
    canvas.drawRect(0, 0, outW, outH);
    canvas.detachBrush();

    const srcPoints: common2D.Point[] = [
      { x: px[0] - minX, y: px[1] - minY },
      { x: px[2] - minX, y: px[3] - minY },
      { x: px[4] - minX, y: px[5] - minY },
      { x: px[6] - minX, y: px[7] - minY }
    ];
    const dstPoints: common2D.Point[] = [
      { x: 0, y: 0 },
      { x: outW, y: 0 },
      { x: outW, y: outH },
      { x: 0, y: outH }
    ];
    const matrix = new drawing.Matrix();
    matrix.setPolyToPoly(srcPoints, dstPoints, 4);
    canvas.setMatrix(matrix);
    canvas.drawImage(srcPm, 0, 0);
    canvas.resetMatrix();

    if (enhanced > 0) {
      await compensateShadow(fullPm, outW, outH, enhanced);
    }

    if (erase.length >= 3) {
      eraseStrokes(canvas, outW, outH, erase);
    }

    let outPm: image.PixelMap = fullPm;
    let resW = outW;
    let resH = outH;
    if (applyCrop && crop.length === 4) {
      const x1 = Math.max(0, Math.min(1, crop[0]));
      const y1 = Math.max(0, Math.min(1, crop[1]));
      const x2 = Math.max(0, Math.min(1, crop[2]));
      const y2 = Math.max(0, Math.min(1, crop[3]));
      const cw = Math.round((x2 - x1) * outW);
      const ch = Math.round((y2 - y1) * outH);
      if (cw >= 8 && ch >= 8 && (cw < outW || ch < outH)) {
        cropPm = image.createPixelMapSync({
          size: { width: cw, height: ch },
          pixelFormat: image.PixelMapFormat.RGBA_8888,
          alphaType: image.AlphaType.OPAQUE,
          editable: true
        });
        const cropCanvas = new drawing.Canvas(cropPm);
        const srcRect: common2D.Rect = {
          left: x1 * outW,
          top: y1 * outH,
          right: x2 * outW,
          bottom: y2 * outH
        };
        const dstRect: common2D.Rect = { left: 0, top: 0, right: cw, bottom: ch };
        cropCanvas.drawImageRectWithSrc(fullPm, srcRect, dstRect);
        outPm = cropPm;
        resW = cw;
        resH = ch;
      }
    }

    const packer = image.createImagePacker();
    const packing: image.PackingOption = { format: 'image/jpeg', quality: 85 };
    const outBytes = await packer.packing(outPm, packing);
    writeBytes(outPath, outBytes);
    await packer.release();

    if (srcPm !== undefined) {
      await srcPm.release();
      srcPm = undefined;
    }
    if (cropPm !== undefined) {
      await cropPm.release();
      cropPm = undefined;
    }
    await fullPm.release();
    fullPm = undefined;
    if (source !== undefined) {
      await source.release();
      source = undefined;
    }

    const ok: WarpResult = { ok: true, width: resW, height: resH, message: '' };
    return ok;
  } catch (err) {
    const e = err as Error;
    if (cropPm !== undefined) {
      try {
        await cropPm.release();
      } catch (ignore) {
      }
    }
    if (fullPm !== undefined) {
      try {
        await fullPm.release();
      } catch (ignore) {
      }
    }
    if (srcPm !== undefined) {
      try {
        await srcPm.release();
      } catch (ignore) {
      }
    }
    if (source !== undefined) {
      try {
        await source.release();
      } catch (ignore) {
      }
    }
    const bad: WarpResult = { ok: false, width: 0, height: 0, message: e.message };
    return bad;
  }
}
