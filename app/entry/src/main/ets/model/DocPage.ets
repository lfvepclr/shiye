/** 单页状态。与服务端 `pages.status` 对齐，另加本地 `local`。 */
export class PageStatus {
  static readonly LOCAL: string = 'local';
  static readonly PENDING: string = 'pending';
  static readonly UPLOADING: string = 'uploading';
  static readonly RECOGNIZING: string = 'recognizing';
  static readonly DONE: string = 'done';
  static readonly FAILED: string = 'failed';
}

/** 一页文档。`quad` 是归一化四角（左上、右上、右下、左下），共 8 个数。 */
@ObservedV2
export class DocPage {
  readonly id: string;
  @Trace remoteId: string = '';
  @Trace ord: number = 0;
  @Trace status: string = PageStatus.LOCAL;
  @Trace error: string = '';

  /** 拍摄/导入的原图。 */
  @Trace originPath: string = '';
  /** 端侧矫正后的正式图（上传用的就是它）。 */
  @Trace workPath: string = '';
  /** 编辑时的低分辨率预览图。 */
  @Trace previewPath: string = '';
  @Trace thumbPath: string = '';

  /** 原图宽高（像素）。 */
  @Trace w: number = 0;
  @Trace h: number = 0;

  @Trace quad: number[] = [0, 0, 1, 0, 1, 1, 0, 1];
  /** 归一化裁剪框 `[x1, y1, x2, y2]`，相对矫正后的整幅图。 */
  @Trace crop: number[] = [0, 0, 1, 1];
  /** 归一化擦除笔迹 `[x, y, r, ...]`，`x < 0` 表示抬笔。 */
  @Trace erase: number[] = [];
  /** 增强强度 0~100。 */
  @Trace enhanced: number = 60;
  /** 0 / 90 / 180 / 270。 */
  @Trace rotation: number = 0;

  @Trace uploaded: boolean = false;
  @Trace selected: boolean = false;

  constructor(id: string) {
    this.id = id;
  }

  /** 四角是否仍是整幅画面（用于判断要不要跑矫正）。 */
  isFullFrame(): boolean {
    const q = this.quad;
    if (q.length !== 8) {
      return true;
    }
    return q[0] === 0 && q[1] === 0 && q[2] === 1 && q[3] === 0 &&
      q[4] === 1 && q[5] === 1 && q[6] === 0 && q[7] === 1;
  }

  resetQuad(): void {
    this.quad = [0, 0, 1, 0, 1, 1, 0, 1];
    this.rotation = 0;
  }

  /** 裁剪框是否仍是整幅（用于判断要不要真的裁）。 */
  isFullCrop(): boolean {
    const c = this.crop;
    if (c.length !== 4) {
      return true;
    }
    return c[0] === 0 && c[1] === 0 && c[2] === 1 && c[3] === 1;
  }

  /** 是否做过裁剪或擦除（决定要不要重算正式图）。 */
  hasRetouch(): boolean {
    return !this.isFullCrop() || this.erase.length > 0;
  }

  statusText(): string {
    if (this.status === PageStatus.DONE) {
      return '已完成';
    }
    if (this.status === PageStatus.FAILED) {
      return '识别失败';
    }
    if (this.status === PageStatus.RECOGNIZING) {
      return '识别中';
    }
    if (this.status === PageStatus.UPLOADING) {
      return '上传中';
    }
    if (this.status === PageStatus.LOCAL) {
      return '待上传';
    }
    return '待识别';
  }
}

/** 落盘用的纯数据结构。 */
export interface PageJson {
  id: string;
  remoteId: string;
  ord: number;
  status: string;
  error: string;
  originPath: string;
  workPath: string;
  previewPath: string;
  thumbPath: string;
  w: number;
  h: number;
  quad: number[];
  crop?: number[];
  erase?: number[];
  enhanced: number;
  rotation: number;
  uploaded: boolean;
}

export function pageToJson(p: DocPage): PageJson {
  const json: PageJson = {
    id: p.id,
    remoteId: p.remoteId,
    ord: p.ord,
    status: p.status,
    error: p.error,
    originPath: p.originPath,
    workPath: p.workPath,
    previewPath: p.previewPath,
    thumbPath: p.thumbPath,
    w: p.w,
    h: p.h,
    quad: p.quad.slice(0),
    crop: p.crop.slice(0),
    erase: p.erase.slice(0),
    enhanced: p.enhanced,
    rotation: p.rotation,
    uploaded: p.uploaded
  };
  return json;
}

export function pageFromJson(json: PageJson): DocPage {
  const page = new DocPage(json.id);
  page.remoteId = json.remoteId;
  page.ord = json.ord;
  page.status = json.status;
  page.error = json.error;
  page.originPath = json.originPath;
  page.workPath = json.workPath;
  page.previewPath = json.previewPath;
  page.thumbPath = json.thumbPath;
  page.w = json.w;
  page.h = json.h;
  page.quad = json.quad !== undefined ? json.quad.slice(0) : [0, 0, 1, 0, 1, 1, 0, 1];
  page.crop = json.crop !== undefined ? json.crop.slice(0) : [0, 0, 1, 1];
  page.erase = json.erase !== undefined ? json.erase.slice(0) : [];
  page.enhanced = json.enhanced;
  page.rotation = json.rotation;
  page.uploaded = json.uploaded;
  return page;
}
