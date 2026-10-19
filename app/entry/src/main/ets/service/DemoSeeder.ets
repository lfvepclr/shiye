import { image } from '@kit.ImageKit';
import { drawing } from '@kit.ArkGraphics2D';
import { ApiClient } from './ApiClient';
import { FileService } from './FileService';
import { ImagePipeline } from './ImagePipeline';
import { GroupStore } from '../store/GroupStore';
import { DocPage, PageStatus } from '../model/DocPage';
import { Time } from '../utils/Time';

/**
 * 调试用演示页：生成一张带文字框与单侧阴影的「纸」，走和拍照完全相同的
 * 落盘 → 矫正 → 上传链路。只在 rawfile/config.json 里 `demoMode: true` 时可用。
 */
export class DemoSeeder {
  static async seed(): Promise<string> {
    const groupStore = GroupStore.get();
    const dto = await ApiClient.createGroup(`演示 ${Time.groupTitle(Time.now())}`);
    const group = groupStore.newGroup(dto.id, dto.title);
    groupStore.add(group);

    const pageId = `demo-${Date.now()}`;
    const origin = FileService.originPath(group.id, pageId);
    FileService.write(origin, await DemoSeeder.makePage());

    const page = new DocPage(pageId);
    page.originPath = origin;
    page.w = 1200;
    page.h = 1600;
    page.status = PageStatus.LOCAL;
    groupStore.addPage(group, page);
    await ImagePipeline.refreshThumb(group.id, page);
    groupStore.touch(group);
    return group.id;
  }

  /** Brush 在 attach 时被 canvas 拷贝，改色后必须重新 attach。 */
  private static fill(canvas: drawing.Canvas, brush: drawing.Brush, color: number,
    left: number, top: number, right: number, bottom: number): void {
    brush.setColor(color);
    canvas.attachBrush(brush);
    canvas.drawRect(left, top, right, bottom);
    canvas.detachBrush();
  }

  private static async makePage(): Promise<ArrayBuffer> {
    const width = 1200;
    const height = 1600;
    const options: image.InitializationOptions = {
      size: { width: width, height: height },
      pixelFormat: image.PixelMapFormat.RGBA_8888,
      alphaType: image.AlphaType.OPAQUE,
      editable: true
    };
    const pm = image.createPixelMapSync(options);
    const canvas = new drawing.Canvas(pm);
    const brush = new drawing.Brush();

    DemoSeeder.fill(canvas, brush, 0xFFFDFBF6, 0, 0, width, height);
    // 右侧阴影，用来验证增强
    DemoSeeder.fill(canvas, brush, 0x22000000, width * 0.72, 0, width, height);

    // 「文字行」
    let y = 140;
    let line = 0;
    while (y < height - 160) {
      const w = line % 5 === 4 ? width * 0.42 : width * 0.78;
      DemoSeeder.fill(canvas, brush, 0xFF1A1A1A, 120, y, 120 + w, y + 18);
      y += 46;
      line++;
    }

    const packer = image.createImagePacker();
    const packing: image.PackingOption = { format: 'image/jpeg', quality: 92 };
    const bytes = await packer.packing(pm, packing);
    await packer.release();
    await pm.release();
    return bytes;
  }
}
