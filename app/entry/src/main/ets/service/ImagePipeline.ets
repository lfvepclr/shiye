import { taskpool } from '@kit.ArkTS';
import { DocPage } from '../model/DocPage';
import { Constants } from '../common/Constants';
import { FileService } from './FileService';
import { WarpResult, warpAndSave } from '../tasks/WarpTask';
import { makeThumb } from '../tasks/IngestTask';
import { Logger } from '../utils/Logger';

/** 图像链路：TaskPool 里做矫正 / 编码，主线程只拿结果路径。 */
export class ImagePipeline {
  private static async warp(srcPath: string, outPath: string, page: DocPage,
    edge: number, applyCrop: boolean, applyErase: boolean): Promise<WarpResult> {
    // @Trace 数组是 Proxy，TaskPool 不能序列化，必须拷成普通数组
    const quad: number[] = [];
    for (let i = 0; i < page.quad.length; i++) {
      quad.push(page.quad[i]);
    }
    const crop: number[] = [];
    for (let i = 0; i < page.crop.length; i++) {
      crop.push(page.crop[i]);
    }
    const erase: number[] = [];
    if (applyErase) {
      for (let i = 0; i < page.erase.length; i++) {
        erase.push(page.erase[i]);
      }
    }
    const enhanced = page.enhanced;
    const task = new taskpool.Task(warpAndSave, srcPath, outPath, quad, enhanced, edge,
      crop, erase, applyCrop);
    const result = await taskpool.execute(task) as WarpResult;
    return result;
  }

  /**
   * 编辑时的低分辨率预览。
   *
   * `full` 为 true 时只做矫正 + 增强，不裁不擦——裁剪框和笔迹要画在完整图上，
   * 否则坐标会错位。默认输出最终效果（裁剪 + 擦除）。
   */
  static async preview(groupId: string, page: DocPage, full: boolean = false): Promise<boolean> {
    if (page.originPath.length === 0) {
      return false;
    }
    const out = full
      ? FileService.previewFullPath(groupId, page.id)
      : FileService.previewPath(groupId, page.id);
    // 完整预览（full）只保留裁剪前的画幅，但擦除要画上去：
    // 擦除笔迹用的是完整图坐标，不影响裁剪框与四角手柄的对位。
    const result = await ImagePipeline.warp(page.originPath, out, page,
      Constants.PREVIEW_EDGE, !full, true);
    if (!result.ok) {
      Logger.warn('Pipeline', `预览失败: ${result.message}`);
      return false;
    }
    if (!full) {
      page.previewPath = out;
    }
    return true;
  }

  /** 保存：2000px 正式参数重算（含裁剪与擦除），并刷新缩略图。 */
  static async finalize(groupId: string, page: DocPage): Promise<boolean> {
    if (page.originPath.length === 0) {
      return false;
    }
    const work = FileService.workPath(groupId, page.id);
    const result = await ImagePipeline.warp(page.originPath, work, page,
      Constants.EXPORT_EDGE, true, true);
    if (!result.ok) {
      Logger.error('Pipeline', `导出失败: ${result.message}`);
      return false;
    }
    page.workPath = work;
    page.w = result.width;
    page.h = result.height;

    const thumb = FileService.thumbPath(groupId, page.id);
    const thumbTask = new taskpool.Task(makeThumb, work, thumb);
    const thumbOk = await taskpool.execute(thumbTask) as boolean;
    if (thumbOk) {
      page.thumbPath = thumb;
    }
    return true;
  }

  /** 只重算缩略图（拍摄直出用）。 */
  static async refreshThumb(groupId: string, page: DocPage): Promise<void> {
    const src = page.workPath.length > 0 ? page.workPath : page.originPath;
    if (src.length === 0) {
      return;
    }
    const thumb = FileService.thumbPath(groupId, page.id);
    const task = new taskpool.Task(makeThumb, src, thumb);
    const ok = await taskpool.execute(task) as boolean;
    if (ok) {
      page.thumbPath = thumb;
    }
  }
}
