import { camera } from '@kit.CameraKit';
import { common } from '@kit.AbilityKit';
import { image } from '@kit.ImageKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { Logger } from '../utils/Logger';

/**
 * 自建相机会话：XComponent surface + PreviewOutput + PhotoOutput。
 *
 * 这里刻意不使用 cameraPicker：只有自建会话才能连拍不打断、并拿到原始 JPEG 字节。
 */
export class CameraService {
  private manager: camera.CameraManager | undefined = undefined;
  private input: camera.CameraInput | undefined = undefined;
  private preview: camera.PreviewOutput | undefined = undefined;
  private photo: camera.PhotoOutput | undefined = undefined;
  private session: camera.PhotoSession | undefined = undefined;
  private surfaceId: string = '';
  private facing: camera.CameraPosition = camera.CameraPosition.CAMERA_POSITION_BACK;
  private flashMode: camera.FlashMode = camera.FlashMode.FLASH_MODE_AUTO;

  onPhoto: (bytes: ArrayBuffer) => void = (bytes: ArrayBuffer) => {
  };
  onError: (message: string) => void = (message: string) => {
  };
  onReady: () => void = () => {
  };

  init(ctx: common.UIAbilityContext): void {
    if (this.manager === undefined) {
      this.manager = camera.getCameraManager(ctx);
    }
  }

  isOpen(): boolean {
    return this.session !== undefined;
  }

  async open(surfaceId: string): Promise<void> {
    this.surfaceId = surfaceId;
    await this.buildSession();
  }

  private pickProfile(profiles: camera.Profile[], preferWide: boolean): camera.Profile {
    let best = profiles[0];
    for (let i = 0; i < profiles.length; i++) {
      const p = profiles[i];
      if (p.size.width > best.size.width) {
        best = p;
      }
    }
    // 预览优先 16:9，避免铺满时被裁太多
    if (preferWide) {
      for (let i = 0; i < profiles.length; i++) {
        const p = profiles[i];
        const ratio = p.size.width / p.size.height;
        if (ratio > 1.7 && ratio < 1.85 && p.size.width >= 1080) {
          return p;
        }
      }
    }
    return best;
  }

  private async buildSession(): Promise<void> {
    const manager = this.manager;
    if (manager === undefined) {
      throw new Error('相机未初始化');
    }
    if (this.surfaceId.length === 0) {
      throw new Error('预览画布未就绪');
    }

    const devices = manager.getSupportedCameras();
    let device: camera.CameraDevice | undefined = undefined;
    for (let i = 0; i < devices.length; i++) {
      if (devices[i].cameraPosition === this.facing) {
        device = devices[i];
        break;
      }
    }
    if (device === undefined) {
      throw new Error('没有可用的摄像头');
    }

    const capability = manager.getSupportedOutputCapability(device, camera.SceneMode.NORMAL_PHOTO);
    if (capability.previewProfiles.length === 0 || capability.photoProfiles.length === 0) {
      throw new Error('摄像头能力不支持拍照');
    }

    const input = manager.createCameraInput(device);
    await input.open();

    const previewProfile = this.pickProfile(capability.previewProfiles, true);
    const photoProfile = this.pickProfile(capability.photoProfiles, false);
    const preview = manager.createPreviewOutput(previewProfile, this.surfaceId);
    const photo = manager.createPhotoOutput(photoProfile);

    photo.on('photoAvailable', (err: BusinessError, p: camera.Photo) => {
      Logger.info('Camera', 'photoAvailable');
      if (err !== undefined && err.code !== 0) {
        this.onError(`拍照失败: ${err.message}`);
        return;
      }
      const imageObj = p.main;
      let done = false;
      const timer = setTimeout(() => {
        if (done) {
          return;
        }
        done = true;
        p.release();
        Logger.error('Camera', '取图超时：设备未返回 JPEG 分量');
        this.onError('相机未返回图片数据，请重试');
      }, 2500);

      imageObj.getComponent(image.ComponentType.JPEG).then((component: image.Component) => {
        if (done) {
          return;
        }
        done = true;
        clearTimeout(timer);
        // 拷一份，回调结束后底层缓冲会被复用
        const bytes = component.byteBuffer.slice(0);
        Logger.info('Camera', `JPEG ${bytes.byteLength} bytes`);
        p.release();
        this.onPhoto(bytes);
      }).catch((e: BusinessError) => {
        if (done) {
          return;
        }
        done = true;
        clearTimeout(timer);
        Logger.error('Camera', `取图失败: ${e.code} ${e.message}`);
        p.release();
        this.onError(`取图失败: ${e.message}`);
      });
    });

    const session = manager.createSession<camera.PhotoSession>(camera.SceneMode.NORMAL_PHOTO);
    session.beginConfig();
    session.addInput(input);
    session.addOutput(preview);
    session.addOutput(photo);
    await session.commitConfig();
    await session.start();

    this.input = input;
    this.preview = preview;
    this.photo = photo;
    this.session = session;

    this.applyFlash();
    Logger.info('Camera', '会话已启动');
    this.onReady();
  }

  async capture(): Promise<void> {
    const photo = this.photo;
    if (photo === undefined) {
      throw new Error('相机未就绪');
    }
    await photo.capture();
  }

  private applyFlash(): void {
    const session = this.session;
    if (session === undefined) {
      return;
    }
    try {
      if (session.hasFlash()) {
        session.setFlashMode(this.flashMode);
      }
    } catch (err) {
      Logger.warn('Camera', '设置闪光灯失败');
    }
  }

  setFlashMode(mode: camera.FlashMode): void {
    this.flashMode = mode;
    this.applyFlash();
  }

  setZoom(ratio: number): void {
    const session = this.session;
    if (session === undefined) {
      return;
    }
    try {
      session.setZoomRatio(ratio);
    } catch (err) {
      Logger.warn('Camera', '设置变焦失败');
    }
  }

  setFocus(x: number, y: number): void {
    const session = this.session;
    if (session === undefined) {
      return;
    }
    const point: camera.Point = { x: x, y: y };
    try {
      session.setFocusPoint(point);
    } catch (err) {
      Logger.warn('Camera', '对焦失败');
    }
  }

  /** 前后摄切换：只重建 input + preview，保留 photoOutput。 */
  async switchCamera(): Promise<void> {
    const next = this.facing === camera.CameraPosition.CAMERA_POSITION_BACK
      ? camera.CameraPosition.CAMERA_POSITION_FRONT
      : camera.CameraPosition.CAMERA_POSITION_BACK;
    await this.releaseSession();
    this.facing = next;
    await this.buildSession();
  }

  private async releaseSession(): Promise<void> {
    const session = this.session;
    this.session = undefined;
    if (session !== undefined) {
      try {
        await session.stop();
        await session.release();
      } catch (err) {
        Logger.warn('Camera', '释放会话失败');
      }
    }
    if (this.preview !== undefined) {
      try {
        await this.preview.release();
      } catch (err) {
      }
      this.preview = undefined;
    }
    if (this.photo !== undefined) {
      try {
        await this.photo.release();
      } catch (err) {
      }
      this.photo = undefined;
    }
    if (this.input !== undefined) {
      try {
        await this.input.close();
      } catch (err) {
      }
      this.input = undefined;
    }
  }

  async close(): Promise<void> {
    await this.releaseSession();
  }
}
