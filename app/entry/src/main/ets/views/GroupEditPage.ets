import { promptAction } from '@kit.ArkUI';
import { Theme } from '../common/Theme';
import { GroupStore } from '../store/GroupStore';
import { DocGroup } from '../model/DocGroup';
import { DocPage } from '../model/DocPage';
import { FileService } from '../service/FileService';
import { ImagePipeline } from '../service/ImagePipeline';
import { UploadQueue } from '../service/UploadQueue';
import { ApiClient, ApiError } from '../service/ApiClient';
import { Logger } from '../utils/Logger';

/** P3 组编辑页：3 列网格、长按拖拽排序、生成 Word。 */
@ComponentV2
export struct GroupEditPage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local group: DocGroup | undefined = undefined;
  @Local selecting: boolean = false;
  @Local sorting: boolean = false;
  @Local editingTitle: boolean = false;
  @Local draftTitle: string = '';
  @Local generating: boolean = false;
  @Local progressText: string = '';
  @Local dragPage: DocPage | undefined = undefined;

  aboutToAppear(): void {
    this.group = this.store.current();
    if (this.group !== undefined) {
      this.draftTitle = this.group.title;
    }
  }

  aboutToDisappear(): void {
    this.store.flush();
  }

  private uri(path: string): string {
    return path.length > 0 ? `file://${path}` : '';
  }

  private pages(): DocPage[] {
    return this.group !== undefined ? this.group.pages : [];
  }

  private selectedCount(): number {
    let n = 0;
    const list = this.pages();
    for (let i = 0; i < list.length; i++) {
      if (list[i].selected) {
        n++;
      }
    }
    return n;
  }

  private toggleSelect(page: DocPage): void {
    page.selected = !page.selected;
    const count = this.selectedCount();
    if (count === 0) {
      this.selecting = false;
    }
  }

  private async commitTitle(): Promise<void> {
    const group = this.group;
    this.editingTitle = false;
    if (group === undefined) {
      return;
    }
    const title = this.draftTitle.trim();
    if (title.length === 0 || title === group.title) {
      this.draftTitle = group.title;
      return;
    }
    group.title = title;
    this.store.touch(group);
    try {
      await ApiClient.patchGroup(group.id, title, []);
    } catch (err) {
      Logger.warn('GroupEdit', '改名同步失败');
    }
  }

  private movePage(from: number, to: number): void {
    const group = this.group;
    if (group === undefined || from === to) {
      return;
    }
    const ids: string[] = [];
    for (let i = 0; i < group.pages.length; i++) {
      ids.push(group.pages[i].id);
    }
    if (from < 0 || from >= ids.length || to < 0 || to > ids.length) {
      return;
    }
    const moved = ids.splice(from, 1);
    if (moved.length === 0) {
      return;
    }
    const target = to > from ? to - 1 : to;
    ids.splice(target, 0, moved[0]);
    group.reorder(ids);
    this.store.touch(group);
    this.syncOrder();
  }

  /** 排序模式下的单步移动，比拖拽更可控。 */
  private stepPage(index: number, delta: number): void {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    const to = index + delta;
    if (to < 0 || to >= group.pages.length) {
      return;
    }
    const ids: string[] = [];
    for (let i = 0; i < group.pages.length; i++) {
      ids.push(group.pages[i].id);
    }
    const moved = ids.splice(index, 1);
    if (moved.length === 0) {
      return;
    }
    ids.splice(to, 0, moved[0]);
    group.reorder(ids);
    this.store.touch(group);
    this.syncOrder();
  }

  private async syncOrder(): Promise<void> {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    const order = group.remotePageIds();
    if (order.length === 0) {
      return;
    }
    try {
      await ApiClient.patchGroup(group.id, '', order);
    } catch (err) {
      Logger.warn('GroupEdit', '页序同步失败');
    }
  }

  private async deleteSelected(): Promise<void> {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    const targets: DocPage[] = [];
    for (let i = 0; i < group.pages.length; i++) {
      if (group.pages[i].selected) {
        targets.push(group.pages[i]);
      }
    }
    for (let i = 0; i < targets.length; i++) {
      const page = targets[i];
      if (page.remoteId.length > 0) {
        try {
          await ApiClient.deletePage(page.remoteId);
        } catch (err) {
          Logger.warn('GroupEdit', '服务端删页失败');
        }
      }
      FileService.removePage(group.id, page.id);
      this.store.removePage(group, page.id);
    }
    this.selecting = false;
    this.syncOrder();
  }

  private async generate(): Promise<void> {
    const group = this.group;
    if (group === undefined || group.pageCount() === 0 || this.generating) {
      return;
    }
    this.generating = true;
    this.progressText = '正在处理图片…';
    try {
      // 没编辑过的页也要跑一遍矫正，保证上传的是正式图
      for (let i = 0; i < group.pages.length; i++) {
        const page = group.pages[i];
        if (page.workPath.length === 0) {
          this.progressText = `正在处理 ${i + 1} / ${group.pages.length}`;
          const ok = await ImagePipeline.finalize(group.id, page);
          if (!ok) {
            page.status = 'failed';
            page.error = '图像处理失败';
          }
        }
      }
      this.store.touch(group);
      this.progressText = '正在上传…';
      await UploadQueue.run(group, (done: number, total: number) => {
        this.progressText = `正在上传 ${done} / ${total}`;
      });
      this.stack.pushPathByName('progress', '');
    } catch (err) {
      const e = err as ApiError;
      promptAction.showToast({ message: e.message, duration: 3000 });
      group.state = 'failed';
      group.error = e.message;
      this.store.touch(group);
    } finally {
      this.generating = false;
      this.progressText = '';
    }
  }

  @Builder
  dragItemPreview() {
    Stack() {
      if (this.dragPage !== undefined) {
        Image(this.uri(this.dragPage.thumbPath))
          .width(96)
          .height(126)
          .objectFit(ImageFit.Cover)
          .borderRadius(10)
      }
    }
    .width(96)
    .height(126)
    .shadow({ radius: 24, color: 'rgba(26,26,26,0.24)', offsetX: 0, offsetY: 12 })
    .scale({ x: 1.06, y: 1.06 })
  }

  @Builder
  pageCell(page: DocPage, index: number) {
    Stack({ alignContent: Alignment.TopStart }) {
      if (page.thumbPath.length > 0) {
        Image(this.uri(page.thumbPath))
          .width('100%')
          .height('100%')
          .objectFit(ImageFit.Cover)
          .borderRadius(Theme.RADIUS_THUMB)
      } else {
        Column()
          .width('100%')
          .height('100%')
          .backgroundColor('#F1ECE3')
          .borderRadius(Theme.RADIUS_THUMB)
      }

      Text(`${index + 1}`)
        .fontSize(11)
        .fontColor(Color.White)
        .backgroundColor('rgba(26,26,26,0.55)')
        .borderRadius(6)
        .padding({ left: 6, right: 6, top: 2, bottom: 2 })
        .margin(6)

      if (this.selecting || page.selected) {
        Text(page.selected ? '✓' : '')
          .fontSize(12)
          .fontColor(Color.White)
          .width(20)
          .height(20)
          .textAlign(TextAlign.Center)
          .borderRadius(10)
          .backgroundColor(page.selected ? Theme.ACCENT : 'rgba(255,255,255,0.75)')
          .border({ width: 1, color: Color.White })
          .position({ x: '100%', y: 0 })
          .translate({ x: -26, y: 6 })
      }

      if (page.status === 'failed') {
        Text('!')
          .fontSize(12)
          .fontColor(Color.White)
          .width(18)
          .height(18)
          .textAlign(TextAlign.Center)
          .borderRadius(9)
          .backgroundColor(Theme.ACCENT)
          .position({ x: 6, y: '100%' })
          .translate({ y: -24 })
      }

      if (this.sorting) {
        Row() {
          if (index > 0) {
            Text('‹')
              .fontSize(17)
              .fontColor(Theme.INK)
              .width(28)
              .height(28)
              .textAlign(TextAlign.Center)
              .borderRadius(14)
              .backgroundColor('rgba(255,255,255,0.86)')
              .onClick(() => {
                this.stepPage(index, -1);
              })
          } else {
            Text('')
              .width(28)
              .height(28)
          }

          if (index < this.pages().length - 1) {
            Text('›')
              .fontSize(17)
              .fontColor(Theme.INK)
              .width(28)
              .height(28)
              .textAlign(TextAlign.Center)
              .borderRadius(14)
              .backgroundColor('rgba(255,255,255,0.86)')
              .onClick(() => {
                this.stepPage(index, 1);
              })
          } else {
            Text('')
              .width(28)
              .height(28)
          }
        }
        .width('100%')
        .justifyContent(FlexAlign.SpaceBetween)
        .padding({ left: 4, right: 4 })
        .position({ x: 0, y: '100%' })
        .translate({ y: -34 })
      }
    }
    .width('100%')
    .height(126)
    .borderRadius(Theme.RADIUS_THUMB)
    .clip(true)
    .border({
      width: page.selected ? 2 : 0,
      color: Theme.ACCENT
    })
    .onClick(() => {
      if (this.sorting) {
        return;
      }
      if (this.selecting || page.selected) {
        this.toggleSelect(page);
        return;
      }
      const group = this.group;
      if (group !== undefined) {
        this.store.setCurrent(group.id);
        this.store.currentPageId = page.id;
        this.stack.pushPathByName('pageEdit', page.id);
      }
    })
  }

  build() {
    NavDestination() {
      Column() {
        // 顶栏
        Row() {
          Text('‹')
            .fontSize(26)
            .fontColor(Theme.INK)
            .width(40)
            .height(40)
            .textAlign(TextAlign.Center)
            .onClick(() => {
              this.stack.pop();
            })

          if (this.editingTitle) {
            TextInput({ text: this.draftTitle })
              .layoutWeight(1)
              .height(36)
              .fontSize(16)
              .backgroundColor(Theme.CARD)
              .borderRadius(10)
              .onChange((value: string) => {
                this.draftTitle = value;
              })
              .onSubmit(() => {
                this.commitTitle();
              })
          } else {
            Text(this.group !== undefined ? this.group.title : '')
              .fontSize(16)
              .fontWeight(FontWeight.Medium)
              .fontColor(Theme.INK)
              .layoutWeight(1)
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .onClick(() => {
                this.editingTitle = true;
              })
          }

          Text(this.sorting ? '完成' : '排序')
            .fontSize(14)
            .fontColor(Theme.ACCENT)
            .padding({ left: 10, right: 4 })
            .onClick(() => {
              this.sorting = !this.sorting;
              if (this.sorting) {
                this.selecting = false;
              }
            })

          Text(this.selecting ? '取消' : '选择')
            .fontSize(14)
            .fontColor(Theme.ACCENT)
            .padding({ left: 10, right: 4 })
            .onClick(() => {
              this.selecting = !this.selecting;
              if (this.selecting) {
                this.sorting = false;
              }
              if (!this.selecting) {
                const list = this.pages();
                for (let i = 0; i < list.length; i++) {
                  list[i].selected = false;
                }
              }
            })
        }
        .width('100%')
        .padding({ left: 8, right: 12, top: 8, bottom: 8 })

        Text(this.sorting ? '点 ‹ › 调整页序' : `${this.pages().length} 页 · 长按拖动可调整顺序`)
          .fontSize(12)
          .fontColor(Theme.MUTED)
          .width('100%')
          .padding({ left: Theme.PAGE_PADDING, bottom: 10 })

        if (this.pages().length === 0) {
          Column() {
            Text('这一组还没有页面')
              .fontSize(15)
              .fontColor(Theme.MUTED)
            Text('点下方「继续拍」开始')
              .fontSize(12)
              .fontColor(Theme.MUTED)
              .margin({ top: 6 })
          }
          .width('100%')
          .layoutWeight(1)
          .justifyContent(FlexAlign.Center)
        } else {
          Grid() {
            ForEach(this.pages(), (page: DocPage, index: number) => {
              GridItem() {
                this.pageCell(page, index)
              }
            }, (page: DocPage) => page.id)
          }
          .columnsTemplate('1fr 1fr 1fr')
          .columnsGap(8)
          .rowsGap(8)
          .layoutWeight(1)
          .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING })
          .editMode(true)
          .onItemDragStart((event: ItemDragInfo, itemIndex: number) => {
            const list = this.pages();
            this.dragPage = itemIndex < list.length ? list[itemIndex] : undefined;
            return this.dragItemPreview;
          })
          .onItemDrop((event: ItemDragInfo, itemIndex: number, insertIndex: number, isSuccess: boolean) => {
            if (isSuccess) {
              this.movePage(itemIndex, insertIndex);
            }
            this.dragPage = undefined;
          })
        }

        // 底部操作
        Row({ space: 12 }) {
          if (this.selecting) {
            Button(`删除 (${this.selectedCount()})`)
              .layoutWeight(1)
              .height(48)
              .fontSize(15)
              .fontColor(Color.White)
              .backgroundColor(this.selectedCount() > 0 ? Theme.ACCENT : '#D9D2C6')
              .borderRadius(Theme.RADIUS_BUTTON)
              .enabled(this.selectedCount() > 0)
              .onClick(() => {
                this.deleteSelected();
              })
          } else {
            Button('继续拍')
              .layoutWeight(1)
              .height(48)
              .fontSize(15)
              .fontColor(Theme.INK)
              .backgroundColor('#EFEAE1')
              .borderRadius(Theme.RADIUS_BUTTON)
              .onClick(() => {
                this.stack.pushPathByName('camera', '');
              })

            Button(this.generating ? (this.progressText.length > 0 ? this.progressText : '处理中…')
              : `生成 Word (${this.pages().length})`)
              .layoutWeight(1.4)
              .height(48)
              .fontSize(15)
              .fontColor(Color.White)
              .backgroundColor(Theme.ACCENT)
              .borderRadius(Theme.RADIUS_BUTTON)
              .enabled(!this.generating && this.pages().length > 0)
              .onClick(() => {
                this.generate();
              })
          }
        }
        .width('100%')
        .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING, top: 12, bottom: 20 })
      }
      .width('100%')
      .height('100%')
      .backgroundColor(Theme.PAPER)
    }
    .hideTitleBar(true)
    .onBackPressed(() => {
      this.store.flush();
      this.stack.pop();
      return true;
    })
  }
}
