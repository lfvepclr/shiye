import { DocPage, PageJson, pageFromJson, pageToJson } from './DocPage';

export class GroupState {
  static readonly DRAFT: string = 'draft';
  static readonly UPLOADING: string = 'uploading';
  static readonly RECOGNIZING: string = 'recognizing';
  static readonly DONE: string = 'done';
  static readonly FAILED: string = 'failed';
}

/** 一份文档（组）。页序即 Word 页序，以数组顺序为准。 */
@ObservedV2
export class DocGroup {
  readonly id: string;
  @Trace title: string = '';
  @Trace state: string = GroupState.DRAFT;
  @Trace createdAt: number = 0;
  @Trace updatedAt: number = 0;
  @Trace pages: DocPage[] = [];
  @Trace doneCount: number = 0;
  @Trace docxRev: number = 0;
  @Trace error: string = '';
  @Trace shareUrl: string = '';
  @Trace shareToken: string = '';

  constructor(id: string) {
    this.id = id;
  }

  pageCount(): number {
    return this.pages.length;
  }

  /** 是否有正在跑的上传/识别。 */
  busy(): boolean {
    return this.state === GroupState.UPLOADING || this.state === GroupState.RECOGNIZING;
  }

  statusLabel(): string {
    if (this.state === GroupState.DONE) {
      return '已完成';
    }
    if (this.state === GroupState.FAILED) {
      return '失败';
    }
    if (this.state === GroupState.UPLOADING) {
      return '上传中';
    }
    if (this.state === GroupState.RECOGNIZING) {
      return `识别中 ${this.doneCount}/${this.pageCount()}`;
    }
    return '未生成';
  }

  stateColor(): string {
    if (this.state === GroupState.DONE) {
      return '#3A7D5C';
    }
    if (this.state === GroupState.FAILED) {
      return '#C8412F';
    }
    if (this.busy()) {
      return '#B0722A';
    }
    return '#6B6560';
  }

  findPage(pageId: string): DocPage | undefined {
    for (let i = 0; i < this.pages.length; i++) {
      if (this.pages[i].id === pageId) {
        return this.pages[i];
      }
    }
    return undefined;
  }

  /** 以 id 数组重排页序；不在列表里的页保持原顺序追加在后面。 */
  reorder(ids: string[]): void {
    const next: DocPage[] = [];
    for (let i = 0; i < ids.length; i++) {
      const page = this.findPage(ids[i]);
      if (page !== undefined) {
        next.push(page);
      }
    }
    for (let i = 0; i < this.pages.length; i++) {
      const page = this.pages[i];
      let exists = false;
      for (let j = 0; j < next.length; j++) {
        if (next[j].id === page.id) {
          exists = true;
          break;
        }
      }
      if (!exists) {
        next.push(page);
      }
    }
    this.pages = next;
    this.syncOrd();
  }

  syncOrd(): void {
    for (let i = 0; i < this.pages.length; i++) {
      this.pages[i].ord = i;
    }
  }

  remotePageIds(): string[] {
    const ids: string[] = [];
    for (let i = 0; i < this.pages.length; i++) {
      const page = this.pages[i];
      if (page.remoteId.length > 0) {
        ids.push(page.remoteId);
      }
    }
    return ids;
  }

  firstThumb(): string {
    for (let i = 0; i < this.pages.length; i++) {
      if (this.pages[i].thumbPath.length > 0) {
        return this.pages[i].thumbPath;
      }
    }
    return '';
  }
}

export interface GroupJson {
  id: string;
  title: string;
  state: string;
  createdAt: number;
  updatedAt: number;
  doneCount: number;
  docxRev: number;
  error: string;
  shareUrl: string;
  shareToken: string;
  pages: PageJson[];
}

export function groupToJson(g: DocGroup): GroupJson {
  const pages: PageJson[] = [];
  for (let i = 0; i < g.pages.length; i++) {
    pages.push(pageToJson(g.pages[i]));
  }
  const json: GroupJson = {
    id: g.id,
    title: g.title,
    state: g.state,
    createdAt: g.createdAt,
    updatedAt: g.updatedAt,
    doneCount: g.doneCount,
    docxRev: g.docxRev,
    error: g.error,
    shareUrl: g.shareUrl,
    shareToken: g.shareToken,
    pages: pages
  };
  return json;
}

export function groupFromJson(json: GroupJson): DocGroup {
  const group = new DocGroup(json.id);
  group.title = json.title;
  group.state = json.state;
  group.createdAt = json.createdAt;
  group.updatedAt = json.updatedAt;
  group.doneCount = json.doneCount;
  group.docxRev = json.docxRev;
  group.error = json.error;
  group.shareUrl = json.shareUrl;
  group.shareToken = json.shareToken;
  const pages: DocPage[] = [];
  for (let i = 0; i < json.pages.length; i++) {
    pages.push(pageFromJson(json.pages[i]));
  }
  group.pages = pages;
  return group;
}
