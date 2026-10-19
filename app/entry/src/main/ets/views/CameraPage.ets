import { common } from '@kit.AbilityKit';
import { abilityAccessCtrl, Permissions } from '@kit.AbilityKit';
import { promptAction } from '@kit.ArkUI';
import { window } from '@kit.ArkUI';
import { taskpool } from '@kit.ArkTS';
import { camera } from '@kit.CameraKit';
import { photoAccessHelper } from '@kit.MediaLibraryKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { Theme } from '../common/Theme';
import { Constants } from '../common/Constants';
import { GroupStore } from '../store/GroupStore';
import { DocGroup } from '../model/DocGroup';
import { DocPage, PageStatus } from '../model/DocPage';
import { CameraService } from '../service/CameraService';
import { FileService } from '../service/FileService';
import { ImagePipeline } from '../service/ImagePipeline';
import { IngestResult, ingestPhoto } from '../tasks/IngestTask';
import { ApiClient } from '../service/ApiClient';
import { Logger } from '../utils/Logger';

/** P2 相机页：连拍不打断，按下即拍。 */
@ComponentV2
export struct CameraPage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local group: DocGroup | undefined = undefined;
  @Local photoCount: number = 0;
  @Local flashIndex: number = 0;
  @Local shutterDown: boolean = false;
  @Local ready: boolean = false;
  @Local zoom: number = 1;

  private cameraService: CameraService = new CameraService();
  private controller: XComponentController = new XComponentController();
  private inflight: number = 0;
  private captureTimer: number = -1;
  private windowObj: window.Window | undefined = undefined;

  aboutToAppear(): void {
    this.group = this.store.current();
    this.photoCount = this.group !== undefined ? this.group.pageCount() : 0;
    this.setupWindow();
    this.cameraService.onPhoto = (bytes: ArrayBuffer) => {
      this.handlePhoto(bytes);
    };
    this.cameraService.onError = (message: string) => {
      promptAction.showToast({ message: message });
    };
    this.cameraService.onReady = () => {
      this.ready = true;
    };
  }

  aboutToDisappear(): void {
    if (this.captureTimer !== -1) {
      clearTimeout(this.captureTimer);
      this.captureTimer = -1;
    }
    this.cameraService.close();
    this.restoreWindow();
    this.store.flush();
  }

  private async setupWindow(): Promise<void> {
    try {
      const ctx = getContext(this) as common.UIAbilityContext;
      const win = await window.getLastWindow(ctx);
      this.windowObj = win;
      await win.setWindowLayoutFullScreen(true);
      await win.setWindowSystemBarEnable([]);
      await win.setWindowKeepScreenOn(true);
    } catch (err) {
      Logger.warn('Camera', '全屏设置失败');
    }
  }

  private async restoreWindow(): Promise<void> {
    const win = this.windowObj;
    if (win === undefined) {
      return;
    }
    try {
      await win.setWindowLayoutFullScreen(false);
      await win.setWindowSystemBarEnable(['status', 'navigation']);
      await win.setWindowKeepScreenOn(false);
    } catch (err) {
      Logger.warn('Camera', '恢复系统栏失败');
    }
  }

  private async ensurePermission(): Promise<boolean> {
    const ctx = getContext(this) as common.UIAbilityContext;
    const permissions: Permissions[] = ['ohos.permission.CAMERA'];
    const atManager = abilityAccessCtrl.createAtManager();
    try {
      const result = await atManager.requestPermissionsFromUser(ctx, permissions);
      return result.authResults.length > 0 && result.authResults[0] === 0;
    } catch (err) {
      return false;
    }
  }

  private async startCamera(surfaceId: string): Promise<void> {
    const granted = await this.ensurePermission();
    if (!granted) {
      promptAction.showToast({ message: '没有相机权限，请在设置中开启', duration: 3000 });
      return;
    }
    const ctx = getContext(this) as common.UIAbilityContext;
    try {
      this.cameraService.init(ctx);
      await this.cameraService.open(surfaceId);
    } catch (err) {
      const e = err as Error;
      promptAction.showToast({ message: `相机启动失败: ${e.message}`, duration: 3000 });
      Logger.error('Camera', e.message);
    }
  }

  private async shoot(): Promise<void> {
    if (this.inflight >= Constants.INFLIGHT_MAX) {
      return;
    }
    this.inflight = this.inflight + 1;
    try {
      await this.cameraService.capture();
      if (this.captureTimer === -1) {
        // 部分设备/模拟器不会回图，给用户一个明确提示而不是静默卡住
        this.captureTimer = setTimeout(() => {
          this.captureTimer = -1;
          promptAction.showToast({ message: '拍照超时，请重试', duration: 2000 });
        }, 6000);
      }
    } catch (err) {
      const e = err as Error;
      Logger.warn('Camera', `拍照失败: ${e.message}`);
      promptAction.showToast({ message: `拍照失败: ${e.message}` });
    } finally {
      this.inflight = this.inflight - 1;
    }
  }

  private async handlePhoto(bytes: ArrayBuffer): Promise<void> {
    if (this.captureTimer !== -1) {
      clearTimeout(this.captureTimer);
      this.captureTimer = -1;
    }
    const group = this.group;
    if (group === undefined) {
      return;
    }
    await this.ingestBytes(group, bytes);
  }

  /** 相册导入：项目符号与拍照完全一致，只是字节来源不同。 */
  private async importPhotos(): Promise<void> {
    const group = this.group;
    if (group === undefined) {
      return;
    }
    try {
      const options = new photoAccessHelper.PhotoSelectOptions();
      options.MIMEType = photoAccessHelper.PhotoViewMIMETypes.IMAGE_TYPE;
      options.maxSelectNumber = 20;
      const picker = new photoAccessHelper.PhotoViewPicker();
      const result = await picker.select(options);
      for (let i = 0; i < result.photoUris.length; i++) {
        const uri = result.photoUris[i];
        const stat = fs.statSync(uri);
        const buf = new ArrayBuffer(stat.size);
        const file = fs.openSync(uri, fs.OpenMode.READ_ONLY);
        try {
          fs.readSync(file.fd, buf);
        } finally {
          fs.closeSync(file);
        }
        await this.ingestBytes(group, buf);
      }
    } catch (err) {
      const e = err as Error;
      Logger.warn('Camera', `相册导入失败: ${e.message}`);
    }
  }

  private async ingestBytes(group: DocGroup, bytes: ArrayBuffer): Promise<void> {
    const pageId = `${Date.now()}-${Math.floor(Math.random() * 10000)}`;
    const origin = FileService.originPath(group.id, pageId);
    const thumb = FileService.thumbPath(group.id, pageId);

    Logger.info('Camera', 'handlePhoto start');
    const task = new taskpool.Task(ingestPhoto, bytes, origin, thumb);
    task.setTransferList([bytes]);
    let result: IngestResult | undefined = undefined;
    try {
      result = await taskpool.execute(task) as IngestResult;
    } catch (err) {
      Logger.error('Camera', '落盘失败');
      return;
    }
    if (result === undefined || !result.ok) {
      const reason = result !== undefined ? result.message : '任务无返回';
      Logger.error('Camera', `落盘失败: ${reason}`);
      promptAction.showToast({ message: `保存照片失败: ${reason}` });
      return;
    }
    Logger.info('Camera', `落盘完成 ${result.width}x${result.height}`);

    const page = new DocPage(pageId);
    page.originPath = origin;
    page.thumbPath = thumb;
    page.w = result.width;
    page.h = result.height;
    page.status = PageStatus.LOCAL;
    this.store.addPage(group, page);
    this.photoCount = group.pageCount();
  }

  private async closePage(): Promise<void> {
    const group = this.group;
    this.store.flush();
    if (group !== undefined && group.pageCount() === 0) {
      this.store.remove(group.id);
      try {
        await ApiClient.deleteGroup(group.id);
      } catch (err) {
        Logger.warn('Camera', '删除空组失败');
      }
    }
    this.stack.pop();
  }

  private finish(): void {
    this.store.flush();
    const names = this.stack.getAllPathName();
    const prev = names.length >= 2 ? names[names.length - 2] : '';
    if (prev === 'groupEdit') {
      this.stack.pop();
    } else {
      this.stack.replacePathByName('groupEdit', '');
    }
  }

  private toggleFlash(): void {
    this.flashIndex = (this.flashIndex + 1) % 3;
    let mode = camera.FlashMode.FLASH_MODE_AUTO;
    if (this.flashIndex === 1) {
      mode = camera.FlashMode.FLASH_MODE_OPEN;
    } else if (this.flashIndex === 2) {
      mode = camera.FlashMode.FLASH_MODE_CLOSE;
    }
    this.cameraService.setFlashMode(mode);
  }

  private flashLabel(): string {
    if (this.flashIndex === 1) {
      return '闪光 开';
    }
    if (this.flashIndex === 2) {
      return '闪光 关';
    }
    return '闪光 自动';
  }

  private uri(path: string): string {
    return path.length > 0 ? `file://${path}` : '';
  }

  @Builder
  thumbStrip() {
    List({ space: 8 }) {
      ForEach(this.group !== undefined ? this.group.pages : [], (page: DocPage) => {
        ListItem() {
          Image(this.uri(page.thumbPath))
            .width(48)
            .height(64)
            .objectFit(ImageFit.Cover)
            .borderRadius(8)
            .border({ width: 1, color: 'rgba(255,255,255,0.6)' })
            .onClick(() => {
              const group = this.group;
              if (group !== undefined) {
                this.store.setCurrent(group.id);
                this.store.currentPageId = page.id;
                this.stack.pushPathByName('pageEdit', page.id);
              }
            })
        }
      }, (page: DocPage) => page.id)
    }
    .listDirection(Axis.Horizontal)
    .height(64)
    .width('100%')
    .scrollBar(BarState.Off)
  }

  build() {
    NavDestination() {
      Stack() {
        XComponent({
          id: 'shiye-camera',
          type: XComponentType.SURFACE,
          controller: this.controller
        })
          .width('100%')
          .height('100%')
          .onLoad(() => {
            const surfaceId = this.controller.getXComponentSurfaceId();
            this.startCamera(surfaceId);
          })

        // 顶栏
        Row() {
          Text('✕')
            .fontSize(20)
            .fontColor(Color.White)
            .width(44)
            .height(44)
            .textAlign(TextAlign.Center)
            .backgroundColor('rgba(0,0,0,0.28)')
            .borderRadius(22)
            .onClick(() => {
              this.closePage();
            })

          Column() {
            Text(this.group !== undefined ? this.group.title : '')
              .fontSize(14)
              .fontColor(Color.White)
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
            Text(`${this.photoCount} 页`)
              .fontSize(11)
              .fontColor('rgba(255,255,255,0.75)')
              .margin({ top: 2 })
          }
          .layoutWeight(1)
          .margin({ left: 12 })

          Text(`完成 (${this.photoCount})`)
            .fontSize(14)
            .fontColor(Color.White)
            .padding({ left: 14, right: 14, top: 8, bottom: 8 })
            .backgroundColor(this.photoCount > 0 ? Theme.ACCENT : 'rgba(255,255,255,0.2)')
            .borderRadius(999)
            .onClick(() => {
              if (this.photoCount > 0) {
                this.finish();
              }
            })
        }
        .width('100%')
        .padding({ left: 16, right: 16, top: 12 })
        .position({ x: 0, y: 0 })

        // 底部控制区
        Column() {
          if (this.group !== undefined && this.group.pageCount() > 0) {
            this.thumbStrip()
          }

          Row() {
            Text(this.flashLabel())
              .fontSize(12)
              .fontColor(Color.White)
              .padding({ left: 12, right: 12, top: 8, bottom: 8 })
              .backgroundColor('rgba(0,0,0,0.32)')
              .borderRadius(999)
              .onClick(() => {
                this.toggleFlash();
              })

            Blank()

            // 快门
            Stack() {
              Circle()
                .width(Theme.SHUTTER_SIZE)
                .height(Theme.SHUTTER_SIZE)
                .fill(Color.White)
                .opacity(0.18)
              Circle()
                .width(Theme.SHUTTER_SIZE - 12)
                .height(Theme.SHUTTER_SIZE - 12)
                .fill(Color.White)
                .scale({
                  x: this.shutterDown ? 0.92 : 1,
                  y: this.shutterDown ? 0.92 : 1
                })
                .animation({ duration: 120, curve: Curve.EaseOut })
            }
            .onTouch((event: TouchEvent) => {
              if (event.type === TouchType.Down) {
                this.shutterDown = true;
              } else if (event.type === TouchType.Up || event.type === TouchType.Cancel) {
                this.shutterDown = false;
              }
            })
            .onClick(() => {
              this.shoot();
            })

            Blank()

            Row({ space: 8 }) {
              Text('相册')
                .fontSize(12)
                .fontColor(Color.White)
                .padding({ left: 12, right: 12, top: 8, bottom: 8 })
                .backgroundColor('rgba(0,0,0,0.32)')
                .borderRadius(999)
                .onClick(() => {
                  this.importPhotos();
                })

              Text('切换')
                .fontSize(12)
                .fontColor(Color.White)
                .padding({ left: 12, right: 12, top: 8, bottom: 8 })
                .backgroundColor('rgba(0,0,0,0.32)')
                .borderRadius(999)
                .onClick(() => {
                  this.cameraService.switchCamera();
                })
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
          .padding({ bottom: 28, top: 16 })
        }
        .width('100%')
        .position({ x: 0, y: '100%' })
        .translate({ y: '-100%' })

        if (!this.ready) {
          Column() {
            LoadingProgress().width(36).height(36).color(Color.White)
            Text('相机启动中…')
              .fontSize(13)
              .fontColor('rgba(255,255,255,0.8)')
              .margin({ top: 12 })
          }
          .width('100%')
          .height('100%')
          .justifyContent(FlexAlign.Center)
          .backgroundColor('rgba(0,0,0,0.35)')
        }
      }
      .width('100%')
      .height('100%')
      .backgroundColor(Color.Black)
    }
    .hideTitleBar(true)
    .backgroundColor(Color.Black)
    .onBackPressed(() => {
      this.closePage();
      return true;
    })
  }
}
