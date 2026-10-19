import { common } from '@kit.AbilityKit';
import { pasteboard } from '@kit.BasicServicesKit';
import { promptAction } from '@kit.ArkUI';
import { Theme } from '../common/Theme';
import { GroupStore } from '../store/GroupStore';
import { DocGroup } from '../model/DocGroup';
import { ApiClient, ApiError, ShareDto } from '../service/ApiClient';
import { FileService } from '../service/FileService';
import { Logger } from '../utils/Logger';

/** P6 完成页：下载 / 复制链接 / 二维码 / 浏览器预览。 */
@ComponentV2
export struct ResultPage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local group: DocGroup | undefined = undefined;
  @Local downloading: boolean = false;
  @Local creatingShare: boolean = false;
  @Local showQr: boolean = false;
  @Local expiryIndex: number = 1;

  aboutToAppear(): void {
    this.group = this.store.current();
  }

  private expiresIn(): number {
    if (this.expiryIndex === 0) {
      return 7 * 86400;
    }
    if (this.expiryIndex === 2) {
      return 0;
    }
    return 30 * 86400;
  }

  private expiryLabel(index: number): string {
    if (index === 0) {
      return '7 天';
    }
    if (index === 2) {
      return '永久';
    }
    return '30 天';
  }

  private async download(): Promise<void> {
    const group = this.group;
    if (group === undefined || this.downloading) {
      return;
    }
    this.downloading = true;
    try {
      const bytes = await ApiClient.downloadDocx(group.id);
      const ctx = getContext(this) as common.UIAbilityContext;
      const safeName = group.title.replace(/[:\\/\\*?"<>|]/g, '-');
      const uri = await FileService.saveDocx(ctx, safeName, bytes);
      if (uri.length > 0) {
        promptAction.showToast({ message: '已保存到所选位置' });
      }
    } catch (err) {
      const e = err as ApiError;
      promptAction.showToast({ message: e.message, duration: 3000 });
      Logger.error('Result', e.message);
    } finally {
      this.downloading = false;
    }
  }

  private async ensureShare(): Promise<string> {
    const group = this.group;
    if (group === undefined) {
      return '';
    }
    if (group.shareUrl.length > 0) {
      return group.shareUrl;
    }
    this.creatingShare = true;
    try {
      const share = await ApiClient.createShare(group.id, this.expiresIn());
      group.shareUrl = share.url;
      group.shareToken = share.token;
      this.store.touch(group);
      return share.url;
    } catch (err) {
      const e = err as ApiError;
      promptAction.showToast({ message: e.message, duration: 3000 });
      return '';
    } finally {
      this.creatingShare = false;
    }
  }

  private async copyLink(): Promise<void> {
    const url = await this.ensureShare();
    if (url.length === 0) {
      return;
    }
    try {
      const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, url);
      await pasteboard.getSystemPasteboard().setData(data);
      promptAction.showToast({ message: '链接已复制' });
    } catch (err) {
      promptAction.showToast({ message: url, duration: 4000 });
    }
  }

  private async showQrCode(): Promise<void> {
    const url = await this.ensureShare();
    if (url.length === 0) {
      return;
    }
    this.showQr = true;
  }

  private async openBrowser(): Promise<void> {
    const url = await this.ensureShare();
    if (url.length === 0) {
      return;
    }
    const ctx = getContext(this) as common.UIAbilityContext;
    try {
      await ctx.startAbility({
        action: 'ohos.want.action.viewData',
        entities: ['entity.system.browsable'],
        uri: url
      });
    } catch (err) {
      promptAction.showToast({ message: url, duration: 4000 });
    }
  }

  @Builder
  actionCard(title: string, hint: string, accent: boolean, action: () => void) {
    Column() {
      Text(title)
        .fontSize(15)
        .fontWeight(FontWeight.Medium)
        .fontColor(accent ? Color.White : Theme.INK)
      Text(hint)
        .fontSize(11)
        .fontColor(accent ? 'rgba(255,255,255,0.8)' : Theme.MUTED)
        .margin({ top: 6 })
    }
    .layoutWeight(1)
    .height(92)
    .justifyContent(FlexAlign.Center)
    .backgroundColor(accent ? Theme.ACCENT : Theme.CARD)
    .borderRadius(Theme.RADIUS_CARD)
    .shadow(Theme.SHADOW)
    .onClick(() => {
      action();
    })
  }

  build() {
    NavDestination() {
      Stack({ alignContent: Alignment.Center }) {
        Column() {
          Row() {
            Text('‹')
              .fontSize(26)
              .fontColor(Theme.INK)
              .width(40)
              .height(40)
              .textAlign(TextAlign.Center)
              .onClick(() => {
                // 回组编辑页，方便继续调页序 / 改单页后重新生成
                this.stack.replacePathByName('groupEdit', '');
              })
            Blank()
          }
          .width('100%')
          .padding({ left: 8, right: 8, top: 8 })

          Column() {
            Text('✓')
              .fontSize(30)
              .fontColor(Color.White)
              .width(64)
              .height(64)
              .textAlign(TextAlign.Center)
              .backgroundColor('#3A7D5C')
              .borderRadius(32)

            Text(this.group !== undefined ? `《${this.group.title}》` : '已完成')
              .fontSize(18)
              .fontWeight(FontWeight.Medium)
              .fontColor(Theme.INK)
              .margin({ top: 16 })
              .maxLines(2)
              .textAlign(TextAlign.Center)

            Text(this.group !== undefined
              ? `已完成 · ${this.group.pageCount()} 页`
              : '')
              .fontSize(13)
              .fontColor(Theme.MUTED)
              .margin({ top: 6 })
          }
          .width('100%')
          .padding({ top: 24, bottom: 28 })

          Row({ space: 10 }) {
            this.actionCard(this.downloading ? '导出中…' : '下载 Word', '存到文件', true, () => {
              this.download();
            })
            this.actionCard(this.creatingShare ? '生成中…' : '复制链接', '发给别人', false, () => {
              this.copyLink();
            })
            this.actionCard('二维码', '扫码查看', false, () => {
              this.showQrCode();
            })
          }
          .width('100%')
          .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING })

          Row() {
            Text('链接有效期')
              .fontSize(12)
              .fontColor(Theme.MUTED)
            Blank()
            ForEach([0, 1, 2], (index: number) => {
              Text(this.expiryLabel(index))
                .fontSize(12)
                .fontColor(this.expiryIndex === index ? Color.White : Theme.MUTED)
                .backgroundColor(this.expiryIndex === index ? Theme.ACCENT : '#EFEAE1')
                .borderRadius(999)
                .padding({ left: 12, right: 12, top: 6, bottom: 6 })
                .margin({ left: 8 })
                .onClick(() => {
                  this.expiryIndex = index;
                  const group = this.group;
                  if (group !== undefined) {
                    group.shareUrl = '';
                    group.shareToken = '';
                    this.store.touch(group);
                  }
                })
            })
          }
          .width('100%')
          .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING, top: 24 })

          Blank()

          Text('在浏览器中预览')
            .fontSize(13)
            .fontColor(Theme.ACCENT)
            .margin({ bottom: 28 })
            .onClick(() => {
              this.openBrowser();
            })
        }
        .width('100%')
        .height('100%')

        if (this.showQr && this.group !== undefined) {
          Column() {
            Column() {
              Text('扫码查看')
                .fontSize(14)
                .fontColor(Theme.INK)
                .margin({ bottom: 12 })
              QRCode(this.group.shareUrl)
                .width(200)
                .height(200)
                .color(Theme.INK)
              Text('点击任意处关闭')
                .fontSize(11)
                .fontColor(Theme.MUTED)
                .margin({ top: 12 })
            }
            .padding(24)
            .backgroundColor(Theme.CARD)
            .borderRadius(20)
            .shadow({ radius: 32, color: 'rgba(26,26,26,0.18)', offsetX: 0, offsetY: 12 })
          }
          .width('100%')
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor('rgba(26,26,26,0.18)')
          .onClick(() => {
            this.showQr = false;
          })
        }
      }
      .width('100%')
      .height('100%')
      .backgroundColor(Theme.PAPER)
    }
    .hideTitleBar(true)
    .onBackPressed(() => {
      this.stack.replacePathByName('groupEdit', '');
      return true;
    })
  }
}
