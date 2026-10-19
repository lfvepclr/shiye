import { promptAction } from '@kit.ArkUI';
import { Theme } from '../common/Theme';
import { GroupStore } from '../store/GroupStore';
import { DocGroup, GroupState } from '../model/DocGroup';
import { EmptyState } from '../components/EmptyState';
import { StatusChip } from '../components/StatusChip';
import { ApiClient, ApiError } from '../service/ApiClient';
import { Time } from '../utils/Time';
import { Logger } from '../utils/Logger';
import { AppConfig, Constants } from '../common/Constants';
import { DemoSeeder } from '../service/DemoSeeder';

/** P1 首页：组列表 + 拍新文档。 */
@ComponentV2
export struct HomePage {
  @Require @Param stack: NavPathStack;
  @Local store: GroupStore = GroupStore.get();
  @Local creating: boolean = false;
  @Local pressed: boolean = false;

  aboutToAppear(): void {
    this.syncStates();
  }

  /** 回到首页时对齐服务端状态（识别可能在后台完成）。 */
  private async syncStates(): Promise<void> {
    try {
      const remote = await ApiClient.listGroups();
      for (let i = 0; i < remote.length; i++) {
        const dto = remote[i];
        const group = this.store.find(dto.id);
        if (group !== undefined) {
          group.state = dto.state;
          group.doneCount = dto.done_count;
          group.docxRev = dto.docx_rev;
        }
      }
      this.store.persist();
    } catch (err) {
      Logger.warn('Home', '状态同步失败');
    }
  }

  private async newDoc(): Promise<void> {
    if (this.creating) {
      return;
    }
    this.creating = true;
    try {
      const title = Time.groupTitle(Time.now());
      const dto = await ApiClient.createGroup(title);
      const group = this.store.newGroup(dto.id, dto.title);
      this.store.add(group);
      this.stack.pushPathByName('camera', '');
    } catch (err) {
      const e = err as ApiError;
      promptAction.showToast({ message: e.message, duration: 2500 });
      Logger.error('Home', `建组失败: ${e.message}`);
    } finally {
      this.creating = false;
    }
  }

  private open(group: DocGroup): void {
    this.store.setCurrent(group.id);
    if (group.state === GroupState.DONE) {
      this.stack.pushPathByName('result', '');
    } else if (group.busy()) {
      this.stack.pushPathByName('progress', '');
    } else {
      this.stack.pushPathByName('groupEdit', '');
    }
  }

  private async remove(group: DocGroup): Promise<void> {
    this.store.remove(group.id);
    try {
      await ApiClient.deleteGroup(group.id);
    } catch (err) {
      Logger.warn('Home', '服务端删除失败，本地已移除');
    }
  }

  private async seedDemo(): Promise<void> {
    try {
      await DemoSeeder.seed();
      this.stack.pushPathByName('groupEdit', '');
    } catch (err) {
      const e = err as ApiError;
      promptAction.showToast({ message: e.message });
    }
  }

  private uri(path: string): string {
    return path.length > 0 ? `file://${path}` : '';
  }

  @Builder
  deleteAction(group: DocGroup) {
    Button('删除')
      .height('100%')
      .width(80)
      .fontSize(14)
      .fontColor(Color.White)
      .backgroundColor(Theme.ACCENT)
      .borderRadius(14)
      .onClick(() => {
        this.remove(group);
      })
  }

  @Builder
  groupCard(group: DocGroup) {
    Row() {
      Stack() {
        if (group.firstThumb().length > 0) {
          Image(this.uri(group.firstThumb()))
            .width(64)
            .height(84)
            .objectFit(ImageFit.Cover)
            .borderRadius(10)
        } else {
          Column()
            .width(64)
            .height(84)
            .borderRadius(10)
            .backgroundColor('#F1ECE3')
        }
      }
      .width(64)
      .height(84)

      Column() {
        Text(group.title)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(Theme.INK)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .width('100%')

        Text(`${group.pageCount()} 页 · ${Time.human(group.updatedAt)}`)
          .fontSize(12)
          .fontColor(Theme.MUTED)
          .margin({ top: 6 })

        Row() {
          StatusChip({ label: group.statusLabel(), color: group.stateColor() })
        }
        .margin({ top: 10 })
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      .margin({ left: 14 })

      Text('›')
        .fontSize(22)
        .fontColor(Theme.LINE)
    }
    .width('100%')
    .padding(14)
    .backgroundColor(Theme.CARD)
    .borderRadius(Theme.RADIUS_CARD)
    .shadow(Theme.SHADOW)
    .onClick(() => {
      this.open(group);
    })
  }

  build() {
    Stack({ alignContent: Alignment.Bottom }) {
      Column() {
        Row() {
          Column() {
            Text('拾页')
              .fontSize(28)
              .fontWeight(FontWeight.Bold)
              .fontColor(Theme.INK)
            Text('拍完即成篇')
              .fontSize(12)
              .fontColor(Theme.MUTED)
              .margin({ top: 4 })
          }
          .alignItems(HorizontalAlign.Start)

          Blank()

          if (AppConfig.demoMode) {
            Text('演示页')
              .fontSize(12)
              .fontColor(Theme.ACCENT)
              .padding({ left: 10, right: 10, top: 6, bottom: 6 })
              .backgroundColor(Theme.ACCENT_LIGHT)
              .borderRadius(999)
              .margin({ right: 10 })
              .onClick(() => {
                this.seedDemo();
              })
          }

          Text(`${this.store.groups.length}`)
            .fontSize(13)
            .fontColor(Theme.MUTED)
        }
        .width('100%')
        .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING, top: 12, bottom: 8 })

        if (this.store.groups.length === 0) {
          EmptyState({
            title: '还没有文档',
            hint: '拍下第一份文档，它会自己变成 Word'
          })
        } else {
          List({ space: 12 }) {
            ForEach(this.store.groups, (group: DocGroup) => {
              ListItem() {
                this.groupCard(group)
              }
              .swipeAction({ end: this.deleteAction(group) })
            }, (group: DocGroup) => group.id)
          }
          .width('100%')
          .layoutWeight(1)
          .padding({ left: Theme.PAGE_PADDING, right: Theme.PAGE_PADDING })
          .scrollBar(BarState.Off)
          .contentEndOffset(120)
        }
      }
      .width('100%')
      .height('100%')

      // 主 CTA
      Row() {
        Text('拍新文档')
          .fontSize(17)
          .fontWeight(FontWeight.Medium)
          .fontColor(Color.White)
      }
      .height(56)
      .padding({ left: 32, right: 32 })
      .backgroundColor(Theme.ACCENT)
      .borderRadius(28)
      .shadow({ radius: 20, color: 'rgba(200,65,47,0.32)', offsetX: 0, offsetY: 8 })
      .scale({ x: this.pressed ? 0.97 : 1, y: this.pressed ? 0.97 : 1 })
      .animation({ duration: 120, curve: Curve.EaseOut })
      .margin({ bottom: 36 })
      .onTouch((event: TouchEvent) => {
        if (event.type === TouchType.Down) {
          this.pressed = true;
        } else if (event.type === TouchType.Up || event.type === TouchType.Cancel) {
          this.pressed = false;
        }
      })
      .onClick(() => {
        this.newDoc();
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(Theme.PAPER)
  }
}
