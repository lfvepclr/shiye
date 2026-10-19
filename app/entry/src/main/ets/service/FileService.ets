import { common } from '@kit.AbilityKit';
import { fileIo as fs, picker } from '@kit.CoreFileKit';
import { Logger } from '../utils/Logger';

/** 本地文件树：`{filesDir}/g/{group}/{page}/{origin|work|preview|thumb}.jpg`。 */
export class FileService {
  private static root: string = '';

  static init(ctx: common.UIAbilityContext): void {
    FileService.root = ctx.filesDir;
  }

  static pageDir(groupId: string, pageId: string): string {
    return `${FileService.root}/g/${groupId}/${pageId}`;
  }

  static originPath(groupId: string, pageId: string): string {
    return `${FileService.pageDir(groupId, pageId)}/origin.jpg`;
  }

  static workPath(groupId: string, pageId: string): string {
    return `${FileService.pageDir(groupId, pageId)}/work.jpg`;
  }

  static previewPath(groupId: string, pageId: string): string {
    return `${FileService.pageDir(groupId, pageId)}/preview.jpg`;
  }

  /** 未裁剪/未擦除的预览，编辑裁剪框与笔迹时作为底图。 */
  static previewFullPath(groupId: string, pageId: string): string {
    return `${FileService.pageDir(groupId, pageId)}/preview_full.jpg`;
  }

  static thumbPath(groupId: string, pageId: string): string {
    return `${FileService.pageDir(groupId, pageId)}/thumb.jpg`;
  }

  static ensureDir(path: string): void {
    try {
      if (!fs.accessSync(path)) {
        fs.mkdirSync(path, true);
      }
    } catch (err) {
      fs.mkdirSync(path, true);
    }
  }

  static exists(path: string): boolean {
    try {
      return fs.accessSync(path);
    } catch (err) {
      return false;
    }
  }

  static write(path: string, bytes: ArrayBuffer): void {
    const idx = path.lastIndexOf('/');
    if (idx > 0) {
      FileService.ensureDir(path.substring(0, idx));
    }
    const file = fs.openSync(path, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
    try {
      fs.writeSync(file.fd, bytes);
    } finally {
      fs.closeSync(file);
    }
  }

  static read(path: string): ArrayBuffer {
    const stat = fs.statSync(path);
    const buf = new ArrayBuffer(stat.size);
    const file = fs.openSync(path, fs.OpenMode.READ_ONLY);
    try {
      fs.readSync(file.fd, buf);
    } finally {
      fs.closeSync(file);
    }
    return buf;
  }

  static removePage(groupId: string, pageId: string): void {
    const dir = FileService.pageDir(groupId, pageId);
    try {
      if (fs.accessSync(dir)) {
        fs.rmdirSync(dir);
      }
    } catch (err) {
      Logger.warn('File', `清理页面目录失败: ${pageId}`);
    }
  }

  /** 用系统文件选择器把 docx 存到用户能看到的位置。 */
  static async saveDocx(ctx: common.UIAbilityContext, fileName: string, bytes: ArrayBuffer): Promise<string> {
    const options = new picker.DocumentSaveOptions();
    options.newFileNames = [`${fileName}.docx`];
    const viewPicker = new picker.DocumentViewPicker(ctx);
    const uris = await viewPicker.save(options);
    if (uris.length === 0) {
      return '';
    }
    const uri = uris[0];
    const file = fs.openSync(uri, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
    try {
      fs.writeSync(file.fd, bytes);
    } finally {
      fs.closeSync(file);
    }
    return uri;
  }
}
