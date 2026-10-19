import { promptAction } from '@kit.ArkUI';
import { Theme } from '../common/Theme';
import { GroupStore } from '../store/GroupStore';
import { DocGroup } from '../model/DocGroup';
import { DocPage, PageStatus } from '../model/DocPage';
import { ImagePipeline } from '../service/ImagePipeline';
import { FileService } from '../service/FileService';
import { ApiClient } from '../service/ApiClient';
import { Logger } from '../utils/Logger';

/** 单页编辑：四角矫正 / 裁剪 / 擦除 / 增强强度。 */
@ComponentV2
export struct PageEditPage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local group: DocGroup | undefined = undefined;
  @Local page: DocPage | undefined = undefined;
  @Local areaW: number = 0;
  @Local areaH: number = 0;
  @Local showOriginal: boolean = false;
  @Local working: boolean = false;
  @Local sliderValue: number = 60;
  /** 0 矫正 · 1 裁剪 · 2 擦除 */
  @Local mode: number = 0;
  @Local fullReady: boolean = false;
  @Local fullTick: number = 0;

  private startQuad: number[] = [];
  private startCrop: number[] = [];
  private refreshTimer: number = -1;

  private eraseCtx: CanvasRenderingContext2D =
    new CanvasRenderingContext2D(new RenderingContextSettings(true));
  private strokePoints: number[] = [];
  private lastX: number = -1;
  private lastY: number = -1;

  aboutToAppear(): void {
    this.group = this.store.current();
    if (this.group !== undefined) {
      this.page = this.group.findPage(this.store.currentPageId);
    }
    if (this.page !== undefined && this.group !== undefined) {
      this.sliderValue = this.page.enhanced;
      // 底图总是重算：编辑参数可能被旧版本或退出时未保存的预览覆盖
      if (this.page.originPath.length > 0) {
        this.ensureFullPreview();
      }
    }
  }

  aboutToDisappear(): void {
    if (this.refreshTimer !== -1) {
      clearTimeout(this.refreshTimer);
    }
    this.store.flush();
  }

  /** 预览图会覆盖同一路径，带版本号强制刷新图片缓存。 */
  private uri(path: string): string {
    if (path.length === 0) {
      return '';
    }
    return `file://${path}?v=${this.fullTick}`;
  }

  /**
   * 三种模式统一用「未裁剪未擦除」的底图：
   * 四角手柄、裁剪框、笔迹都是相对完整画面的归一化坐标，换成裁剪后的图会整体错位。
   * 裁剪与擦除的成品效果体现在保存后的缩略图与导出图里。
   */
  private displayPath(): string {
    const group = this.group;
    const page = this.page;
    if (group === undefined || page === undefined) {
      return '';
    }
    if (this.showOriginal) {
      return page.originPath;
    }
    if (this.fullReady) {
      return FileService.previewFullPath(group.id, page.id);
    }
    return page.previewPath.length > 0 ? page.previewPath : page.originPath;
  }

  /** 图片按 Contain 排布后的实际显示区域。 */
  private dispW(): number {
    const page = this.page;
    if (page === undefined || this.areaW === 0 || page.w === 0) {
      return 0;
    }
    const areaAspect = this.areaW / this.areaH;
    const imgAspect = page.w / page.h;
    return imgAspect > areaAspect ? this.areaW : this.areaH * imgAspect;
  }

  private dispH(): number {
    const page = this.page;
    if (page === undefined || this.areaH === 0 || page.w === 0) {
      return 0;
    }
    const areaAspect = this.areaW / this.areaH;
    const imgAspect = page.w / page.h;
    return imgAspect > areaAspect ? this.areaW / imgAspect : this.areaH;
  }

  private offsetX(): number {
    return (this.areaW - this.dispW()) / 2;
  }

  private offsetY(): number {
    return (this.areaH - this.dispH()) / 2;
  }

  private pointX(index: number): number {
    const page = this.page;
    if (page === undefined) {
      return 0;
    }
    if (this.mode === 1) {
      const cx = index === 1 || index === 2 ? 2 : 0;
      return this.offsetX() + page.crop[cx] * this.dispW();
    }
    return this.offsetX() + page.quad[index * 2] * this.dispW();
  }

  private pointY(index: number): number {
    const page = this.page;
    if (page === undefined) {
      return 0;
    }
    if (this.mode === 1) {
      const cy = index >= 2 ? 3 : 1;
      return this.offsetY() + page.crop[cy] * this.dispH();
    }
    return this.offsetY() + page.quad[index * 2 + 1] * this.dispH();
  }

  private clamp01(v: number): number {
    return Math.min(1, Math.max(0, v));
  }

  private onHandleStart(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    if (this.mode === 1) {
      this.startCrop = page.crop.slice(0);
    } else {
      this.startQuad = page.quad.slice(0);
    }
  }

  private onHandleMove(index: number, dx: number, dy: number): void {
    const page = this.page;
    if (page === undefined || this.dispW() === 0) {
      return;
    }
    if (this.mode === 1) {
      this.moveCrop(index, dx / this.dispW(), dy / this.dispH());
      return;
    }
    const nx = this.startQuad[index * 2] + dx / this.dispW();
    const ny = this.startQuad[index * 2 + 1] + dy / this.dispH();
    const quad = page.quad.slice(0);
    quad[index * 2] = this.clamp01(nx);
    quad[index * 2 + 1] = this.clamp01(ny);
    page.quad = quad;
  }

  private moveCrop(index: number, ux: number, uy: number): void {
    const page = this.page;
    if (page === undefined || this.startCrop.length !== 4) {
      return;
    }
    const min = 0.06;
    let x1 = this.startCrop[0];
    let y1 = this.startCrop[1];
    let x2 = this.startCrop[2];
    let y2 = this.startCrop[3];
    if (index === 0) {
      x1 = this.clamp01(Math.min(x1 + ux, x2 - min));
      y1 = this.clamp01(Math.min(y1 + uy, y2 - min));
    } else if (index === 1) {
      x2 = this.clamp01(Math.max(x2 + ux, x1 + min));
      y1 = this.clamp01(Math.min(y1 + uy, y2 - min));
    } else if (index === 2) {
      x2 = this.clamp01(Math.max(x2 + ux, x1 + min));
      y2 = this.clamp01(Math.max(y2 + uy, y1 + min));
    } else {
      x1 = this.clamp01(Math.min(x1 + ux, x2 - min));
      y2 = this.clamp01(Math.max(y2 + uy, y1 + min));
    }
    page.crop = [x1, y1, x2, y2];
  }

  private schedulePreview(delay: number): void {
    if (this.refreshTimer !== -1) {
      clearTimeout(this.refreshTimer);
    }
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = -1;
      this.refreshPreview();
    }, delay);
  }

  private async refreshPreview(): Promise<void> {
    const group = this.group;
    const page = this.page;
    if (group === undefined || page === undefined) {
      return;
    }
    const ok = await ImagePipeline.preview(group.id, page, true);
    if (ok) {
      this.fullReady = true;
      this.fullTick = this.fullTick + 1;
    }
  }

  private async ensureFullPreview(): Promise<void> {
    if (this.fullReady) {
      return;
    }
    await this.refreshPreview();
  }

  private setMode(mode: number): void {
    if (this.mode === mode) {
      return;
    }
    this.mode = mode;
    this.ensureFullPreview();
  }

  private rotateQuad(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    const q = page.quad.slice(0);
    // 顺时针 90°：左下→左上、左上→右上、右上→右下、右下→左下
    const next = [q[6], q[7], q[0], q[1], q[2], q[3], q[4], q[5]];
    page.quad = next;
    page.rotation = (page.rotation + 90) % 360;
    this.fullReady = false;
    this.schedulePreview(120);
  }

  private reset(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    page.resetQuad();
    page.crop = [0, 0, 1, 1];
    page.erase = [];
    this.strokePoints = [];
    this.sliderValue = page.enhanced;
    this.fullReady = false;
    this.fullTick = this.fullTick + 1;
    this.redrawErase();
    this.schedulePreview(120);
  }

  private async deletePage(): Promise<void> {
    const group = this.group;
    const page = this.page;
    if (group === undefined || page === undefined) {
      return;
    }
    if (page.remoteId.length > 0) {
      try {
        await ApiClient.deletePage(page.remoteId);
      } catch (err) {
        Logger.warn('PageEdit', '服务端删页失败');
      }
    }
    FileService.removePage(group.id, page.id);
    this.store.removePage(group, page.id);
    this.stack.pop();
  }

  private async save(): Promise<void> {
    const group = this.group;
    const page = this.page;
    if (group === undefined || page === undefined || this.working) {
      return;
    }
    this.working = true;
    try {
      const ok = await ImagePipeline.finalize(group.id, page);
      if (!ok) {
        promptAction.showToast({ message: '保存失败，请重试' });
        return;
      }
      page.status = PageStatus.LOCAL;
      page.uploaded = false;
      this.store.touch(group);
      this.stack.pop();
    } finally {
      this.working = false;
    }
  }

  // ---- 擦除 ----

  private eraseRadius(): number {
    return 0.035;
  }

  private toNormX(localX: number): number {
    const w = this.dispW();
    return w === 0 ? -1 : (localX - this.offsetX()) / w;
  }

  private toNormY(localY: number): number {
    const h = this.dispH();
    return h === 0 ? -1 : (localY - this.offsetY()) / h;
  }

  private eraseStart(localX: number, localY: number): void {
    this.strokePoints = [];
    this.lastX = -1;
    this.lastY = -1;
    this.eraseAppend(localX, localY);
  }

  private eraseAppend(localX: number, localY: number): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    const u = this.toNormX(localX);
    const v = this.toNormY(localY);
    if (u < 0 || u > 1 || v < 0 || v > 1) {
      return;
    }
    // 抽稀：移动不到 0.6% 不记点，避免笔迹膨胀
    if (this.lastX >= 0) {
      const dx = u - this.lastX;
      const dy = v - this.lastY;
      if (dx * dx + dy * dy < 0.00004) {
        return;
      }
    }
    this.lastX = u;
    this.lastY = v;
    this.strokePoints.push(u);
    this.strokePoints.push(v);
    this.strokePoints.push(this.eraseRadius());
    this.redrawErase();
  }

  private eraseEnd(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    if (this.strokePoints.length >= 3) {
      const next = page.erase.slice(0);
      if (next.length > 0) {
        next.push(-1);
        next.push(0);
        next.push(0);
      }
      for (let i = 0; i < this.strokePoints.length; i++) {
        next.push(this.strokePoints[i]);
      }
      page.erase = next;
    }
    this.strokePoints = [];
    this.lastX = -1;
    this.redrawErase();
    // 底图带上擦除结果，切回矫正模式时能看到成品
    this.schedulePreview(260);
  }

  private undoStroke(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    const e = page.erase.slice(0);
    let cut = -1;
    for (let i = 0; i + 1 < e.length; i += 3) {
      if (e[i] < 0) {
        cut = i;
      }
    }
    if (cut < 0) {
      page.erase = [];
    } else {
      page.erase = e.slice(0, cut);
    }
    this.redrawErase();
    this.schedulePreview(200);
  }

  private clearStrokes(): void {
    const page = this.page;
    if (page === undefined) {
      return;
    }
    page.erase = [];
    this.strokePoints = [];
    this.redrawErase();
    this.schedulePreview(200);
  }

  /** 重画遮罩层：白笔迹只是所见即所得的提示，保存时由 TaskPool 落到图上。 */
  private redrawErase(): void {
    const ctx = this.eraseCtx;
    const page = this.page;
    if (page === undefined || this.areaW === 0) {
      return;
    }
    ctx.clearRect(0, 0, this.areaW, this.areaH);
    const w = this.dispW();
    const h = this.dispH();
    if (w === 0) {
      return;
    }
    const ox = this.offsetX();
    const oy = this.offsetY();
    ctx.strokeStyle = '#FFFFFF';
    ctx.fillStyle = '#FFFFFF';
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    const drawStroke = (points: number[]): void => {
      if (points.length < 3) {
        return;
      }
      const radius = points[2] * w;
      ctx.fillStyle = '#FFFFFF';
      ctx.beginPath();
      ctx.arc(ox + points[0] * w, oy + points[1] * h, radius, 0, Math.PI * 2);
      ctx.fill();
      if (points.length >= 6) {
        ctx.strokeStyle = '#FFFFFF';
        ctx.lineWidth = radius * 2;
        ctx.beginPath();
        ctx.moveTo(ox + points[0] * w, oy + points[1] * h);
        for (let i = 3; i + 2 < points.length; i += 3) {
          ctx.lineTo(ox + points[i] * w, oy + points[i + 1] * h);
        }
        ctx.stroke();
      }
    };

    let stroke: number[] = [];
    const stored = page.erase;
    for (let i = 0; i + 2 < stored.length; i += 3) {
      if (stored[i] < 0) {
        drawStroke(stroke);
        stroke = [];
      } else {
        stroke.push(stored[i]);
        stroke.push(stored[i + 1]);
        stroke.push(stored[i + 2]);
      }
    }
    drawStroke(stroke);
    drawStroke(this.strokePoints);
  }

  @Builder
  handle(index: number) {
    Stack() {
      Circle()
        .width(24)
        .height(24)
        .fill(Color.White)
        .stroke(Theme.ACCENT)
        .strokeWidth(3)
    }
    .width(44)
    .height(44)
    .position({ x: this.pointX(index) - 22, y: this.pointY(index) - 22 })
    .gesture(
      PanGesture()
        .onActionStart(() => {
          this.onHandleStart();
        })
        .onActionUpdate((event: GestureEvent) => {
          this.onHandleMove(index, event.offsetX, event.offsetY);
        })
        .onActionEnd(() => {
          if (this.mode === 1) {
            this.schedulePreview(160);
          } else {
            this.schedulePreview(120);
          }
        })
    )
  }

  @Builder
  cropMask() {
    if (this.dispW() > 0) {
      Column()
        .width(this.areaW)
        .height(this.pointY(1))
        .position({ x: 0, y: 0 })
        .backgroundColor('rgba(16,16,16,0.42)')
      Column()
        .width(this.areaW)
        .height(Math.max(0, this.areaH - this.pointY(2)))
        .position({ x: 0, y: this.pointY(2) })
        .backgroundColor('rgba(16,16,16,0.42)')
      Column()
        .width(this.pointX(0))
        .height(Math.max(0, this.pointY(2) - this.pointY(1)))
        .position({ x: 0, y: this.pointY(1) })
        .backgroundColor('rgba(16,16,16,0.42)')
      Column()
        .width(Math.max(0, this.areaW - this.pointX(1)))
        .height(Math.max(0, this.pointY(2) - this.pointY(1)))
        .position({ x: this.pointX(1), y: this.pointY(1) })
        .backgroundColor('rgba(16,16,16,0.42)')
      Column()
        .width(this.pointX(1) - this.pointX(0))
        .height(this.pointY(2) - this.pointY(1))
        .position({ x: this.pointX(0), y: this.pointY(1) })
        .border({ width: 2, color: Color.White })
    }
  }

  @Builder
  modeButton(label: string, value: number) {
    Button(label)
      .layoutWeight(1)
      .height(38)
      .fontSize(14)
      .fontColor(this.mode === value ? Color.White : Theme.INK)
      .backgroundColor(this.mode === value ? Theme.ACCENT : Color.Transparent)
      .borderRadius(999)
      .onClick(() => {
        this.setMode(value);
      })
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

          Text(this.page !== undefined ? `第 ${this.page.ord + 1} 页` : '编辑')
            .fontSize(15)
            .fontWeight(FontWeight.Medium)
            .layoutWeight(1)
            .textAlign(TextAlign.Center)

          Text('✓')
            .fontSize(20)
            .fontColor(Theme.ACCENT)
            .width(40)
            .height(40)
            .textAlign(TextAlign.Center)
            .onClick(() => {
              this.save();
            })
        }
        .width('100%')
        .padding({ left: 8, right: 8, top: 8 })

        // 图片 + 裁剪/擦除覆盖层 + 四角
        Stack() {
          if (this.displayPath().length > 0) {
            Image(this.uri(this.displayPath()))
              .width('100%')
              .height('100%')
              .objectFit(ImageFit.Contain)
          }

          if (this.mode === 1) {
            this.cropMask()
          }

          if (this.mode === 2) {
            Canvas(this.eraseCtx)
              .width('100%')
              .height('100%')
              .onReady(() => {
                this.redrawErase();
              })
              .gesture(
                PanGesture()
                  .onActionStart((event: GestureEvent) => {
                    const p = event.fingerList[0];
                    if (p !== undefined) {
                      this.eraseStart(p.localX, p.localY);
                    }
                  })
                  .onActionUpdate((event: GestureEvent) => {
                    const p = event.fingerList[0];
                    if (p !== undefined) {
                      this.eraseAppend(p.localX, p.localY);
                    }
                  })
                  .onActionEnd(() => {
                    this.eraseEnd();
                  })
              )
          }

          if (this.dispW() > 0 && (this.mode === 0 || this.mode === 1)) {
            this.handle(0)
            this.handle(1)
            this.handle(2)
            this.handle(3)
          }
        }
        .width('100%')
        .layoutWeight(1)
        .onAreaChange((oldValue: Area, newValue: Area) => {
          this.areaW = Number.parseFloat(newValue.width.toString());
          this.areaH = Number.parseFloat(newValue.height.toString());
        })

        Text(this.mode === 0 ? (this.showOriginal ? '当前显示原图' : '拖动四角对齐纸张')
          : (this.mode === 1 ? '拖动四角选择保留区域' : '在要去掉的内容上涂抹'))
          .fontSize(12)
          .fontColor(Theme.MUTED)
          .margin({ bottom: 6 })

        // 工具条
        Column() {
          Row({ space: 4 }) {
            this.modeButton('矫正', 0)
            this.modeButton('裁剪', 1)
            this.modeButton('擦除', 2)
          }
          .width('100%')
          .padding(4)
          .backgroundColor('#EFEAE1')
          .borderRadius(999)

          if (this.mode === 0) {
            Row({ space: 10 }) {
              Button(this.showOriginal ? '预览' : '原图')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  this.showOriginal = !this.showOriginal;
                })

              Button('旋转')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  this.rotateQuad();
                })

              Button('重置')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  this.reset();
                })

              Button('删除')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.ACCENT)
                .backgroundColor(Theme.ACCENT_LIGHT)
                .borderRadius(999)
                .onClick(() => {
                  this.deletePage();
                })
            }
            .width('100%')
            .margin({ top: 10 })
          } else if (this.mode === 1) {
            Row({ space: 10 }) {
              Button('整幅')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  const page = this.page;
                  if (page !== undefined) {
                    page.crop = [0, 0, 1, 1];
                    this.schedulePreview(160);
                  }
                })

              Button('完成')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Color.White)
                .backgroundColor(Theme.ACCENT)
                .borderRadius(999)
                .onClick(() => {
                  this.setMode(0);
                })
            }
            .width('100%')
            .margin({ top: 10 })
          } else {
            Row({ space: 10 }) {
              Button('撤销')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  this.undoStroke();
                })

              Button('清空')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Theme.INK)
                .backgroundColor('#EFEAE1')
                .borderRadius(999)
                .onClick(() => {
                  this.clearStrokes();
                })

              Button('完成')
                .layoutWeight(1)
                .height(40)
                .fontSize(13)
                .fontColor(Color.White)
                .backgroundColor(Theme.ACCENT)
                .borderRadius(999)
                .onClick(() => {
                  this.setMode(0);
                })
            }
            .width('100%')
            .margin({ top: 10 })
          }

          Row() {
            Text('增强')
              .fontSize(12)
              .fontColor(Theme.MUTED)
              .width(34)
            Slider({
              value: this.sliderValue,
              min: 0,
              max: 100,
              step: 1
            })
              .layoutWeight(1)
              .selectedColor(Theme.ACCENT)
              .onChange((value: number, mode: SliderChangeMode) => {
                this.sliderValue = value;
                const page = this.page;
                if (page !== undefined) {
                  page.enhanced = Math.round(value);
                }
                if (mode === SliderChangeMode.End) {
                  this.fullReady = false;
                  this.schedulePreview(200);
                }
              })
            Text(`${Math.round(this.sliderValue)}`)
              .fontSize(12)
              .fontColor(Theme.MUTED)
              .width(28)
              .textAlign(TextAlign.End)
          }
          .width('100%')
          .margin({ top: 8 })
        }
        .width('100%')
        .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING, bottom: 16 })
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
