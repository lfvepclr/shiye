import { image } from '@kit.ImageKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { writeBytes } from './Shared';

export interface IngestResult {
  ok: boolean;
  width: number;
  height: number;
  message: string;
}

/**
 * 拍照回调只把字节丢进来：落原图 + 生成 320px 缩略图。
 * 在 TaskPool 里跑，主线程不碰 IO 与解码。
 */
@Concurrent
export async function ingestPhoto(buffer: ArrayBuffer, originPath: string,
  thumbPath: string): Promise<IngestResult> {
  try {
    writeBytes(originPath, buffer);

    const source = image.createImageSource(buffer);
    const info = await source.getImageInfo();
    const width = info.size.width;
    const height = info.size.height;

    const edge = Math.max(width, height);
    const scale = edge > 320 ? 320 / edge : 1;
    const target: image.Size = {
      width: Math.max(1, Math.round(width * scale)),
      height: Math.max(1, Math.round(height * scale))
    };
    const options: image.DecodingOptions = { desiredSize: target };
    const thumb = await source.createPixelMap(options);
    const packer = image.createImagePacker();
    const packing: image.PackingOption = { format: 'image/jpeg', quality: 75 };
    const thumbBytes = await packer.packing(thumb, packing);
    writeBytes(thumbPath, thumbBytes);

    await packer.release();
    await thumb.release();
    await source.release();

    const result: IngestResult = { ok: true, width: width, height: height, message: '' };
    return result;
  } catch (err) {
    const e = err as Error;
    const result: IngestResult = { ok: false, width: 0, height: 0, message: e.message };
    return result;
  }
}

/** 由矫正后的正式图生成缩略图（上传后用）。 */
@Concurrent
export async function makeThumb(srcPath: string, thumbPath: string): Promise<boolean> {
  let source: image.ImageSource | undefined = undefined;
  let thumb: image.PixelMap | undefined = undefined;
  let packer: image.ImagePacker | undefined = undefined;
  try {
    source = image.createImageSource(srcPath);
    const info = await source.getImageInfo();
    const edge = Math.max(info.size.width, info.size.height);
    const scale = edge > 320 ? 320 / edge : 1;
    const options: image.DecodingOptions = {
      desiredSize: {
        width: Math.max(1, Math.round(info.size.width * scale)),
        height: Math.max(1, Math.round(info.size.height * scale))
      }
    };
    thumb = await source.createPixelMap(options);
    packer = image.createImagePacker();
    const packing: image.PackingOption = { format: 'image/jpeg', quality: 75 };
    const bytes = await packer.packing(thumb, packing);
    writeBytes(thumbPath, bytes);
    return true;
  } catch (err) {
    return false;
  } finally {
    if (packer !== undefined) {
      await packer.release();
    }
    if (thumb !== undefined) {
      await thumb.release();
    }
    if (source !== undefined) {
      await source.release();
    }
  }
}
