import { promptAction } from '@kit.ArkUI';
import { Theme } from '../common/Theme';
import { GroupStore } from '../store/GroupStore';
import { DocGroup, GroupState } from '../model/DocGroup';
import { DocPage } from '../model/DocPage';
import { PollService } from '../service/PollService';
import { ApiClient, StatusDto } from '../service/ApiClient';
import { Logger } from '../utils/Logger';

/** P5 进度页：轮询 + 逐页状态卡片。 */
@ComponentV2
export struct ProgressPage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local group: DocGroup | undefined = undefined;
  @Local done: number = 0;
  @Local total: number = 0;
  @Local stateText: string = '正在识别…';
  @Local failed: boolean = false;

  private poll: PollService | undefined = undefined;

  aboutToAppear(): void {
    this.group = this.store.current();
    const group = this.group;
    if (group === undefined) {
      return;
    }
    this.total = group.pageCount();
    this.done = group.doneCount;
    this.startPolling(group.id);
  }

  aboutToDisappear(): void {
    const poll = this.poll;
    if (poll !== undefined) {
      poll.stop();
    }
    this.store.flush();
  }

  private startPolling(groupId: string): void {
    const poll = new PollService(groupId);
    this.poll = poll;
    poll.start((status: StatusDto) => {
      this.apply(status);
    }, (status: StatusDto) => {
      this.apply(status);
      if (status.state === 'done') {
        this.stack.replacePathByName('result', '');
      } else {
        this.failed = true;
        this.stateText = status.error ? status.error : '识别失败';
      }
    });
  }

  private apply(status: StatusDto): void {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    this.total = status.total;
    this.done = status.done;
    group.state = status.state;
    group.doneCount = status.done;
    group.docxRev = status.docx_rev;
    group.error = status.error ? status.error : '';

    for (let i = 0; i < status.pages.length; i++) {
      const remote = status.pages[i];
      for (let j = 0; j < group.pages.length; j++) {
        const page = group.pages[j];
        if (page.remoteId === remote.id) {
          page.status = remote.status;
          page.error = remote.error ? remote.error : '';
        }
      }
    }

    if (status.state === 'uploading') {
      this.stateText = '正在上传…';
    } else if (status.state === 'recognizing') {
      this.stateText = `正在识别 ${status.done} / ${status.total}`;
    } else if (status.state === 'failed') {
      this.stateText = status.error ? status.error : '识别失败';
    } else if (status.state === 'done') {
      this.stateText = '识别完成';
    }
    this.store.persist();
  }

  private async retry(): Promise<void> {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    this.failed = false;
    this.stateText = '重新提交识别…';
    try {
      await ApiClient.render(group.id);
      group.state = GroupState.RECOGNIZING;
      this.startPolling(group.id);
    } catch (err) {
      const e = err as Error;
      promptAction.showToast({ message: e.message });
      Logger.error('Progress', e.message);
    }
  }

  private uri(path: string): string {
    return path.length > 0 ? `file://${path}` : '';
  }

  @Builder
  pageCard(page: DocPage, index: number) {
    Stack({ alignContent: Alignment.BottomStart }) {
      if (page.thumbPath.length > 0) {
        Image(this.uri(page.thumbPath))
          .width('100%')
          .height('100%')
          .objectFit(ImageFit.Cover)
          .opacity(0.55)
      } else {
        Column()
          .width('100%')
          .height('100%')
          .backgroundColor('#F1ECE3')
      }

      Row() {
        Text(`第 ${index + 1} 页`)
          .fontSize(12)
          .fontColor(Theme.INK)
        Blank()
        Text(page.status === 'done' ? '已完成'
          : (page.status === 'failed' ? '失败' : '识别中…'))
          .fontSize(12)
          .fontColor(page.status === 'done' ? '#3A7D5C'
            : (page.status === 'failed' ? Theme.ACCENT : Theme.MUTED))
      }
      .width('100%')
      .padding(10)
      .backgroundColor('rgba(255,255,255,0.88)')
      .borderRadius(10)
      .margin(8)
    }
    .width('100%')
    .height(120)
    .borderRadius(Theme.RADIUS_CARD)
    .clip(true)
    .shadow(Theme.SHADOW)
    .margin({ bottom: 10 })
  }

  build() {
    NavDestination() {
      Column() {
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
          Text('生成 Word')
            .fontSize(15)
            .fontWeight(FontWeight.Medium)
            .layoutWeight(1)
            .textAlign(TextAlign.Center)
          Blank().width(40)
        }
        .width('100%')
        .padding({ left: 8, right: 8, top: 8 })

        Column() {
          Progress({
            value: this.done,
            total: this.total > 0 ? this.total : 1,
            type: ProgressType.Ring
          })
            .width(92)
            .height(92)
            .color(Theme.ACCENT)
            .backgroundColor('#EFEAE1')
            .style({ strokeWidth: 8 })

          Text(this.stateText)
            .fontSize(15)
            .fontColor(Theme.INK)
            .margin({ top: 16 })

          Text(`${this.done} / ${this.total} 页`)
            .fontSize(12)
            .fontColor(Theme.MUTED)
            .margin({ top: 6 })
        }
        .width('100%')
        .padding({ top: 16, bottom: 20 })

        if (this.failed) {
          Button('重试')
            .height(44)
            .width(140)
            .fontSize(14)
            .fontColor(Color.White)
            .backgroundColor(Theme.ACCENT)
            .borderRadius(Theme.RADIUS_BUTTON)
            .margin({ bottom: 12 })
            .onClick(() => {
              this.retry();
            })
        }

        List({ space: 0 }) {
          ForEach(this.group !== undefined ? this.group.pages : [], (page: DocPage, index: number) => {
            ListItem() {
              this.pageCard(page, index)
            }
          }, (page: DocPage) => page.id)
        }
        .layoutWeight(1)
        .width('100%')
        .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING })
        .scrollBar(BarState.Off)
      }
      .width('100%')
      .height('100%')
      .backgroundColor(Theme.PAPER)
    }
    .hideTitleBar(true)
    .onBackPressed(() => {
      this.stack.pop();
      return true;
    })
  }
}
