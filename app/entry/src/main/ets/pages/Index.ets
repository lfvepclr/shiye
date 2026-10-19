import { common } from '@kit.AbilityKit';
import { Theme } from '../common/Theme';
import { AppConfig } from '../common/Constants';
import { DeviceId } from '../utils/DeviceId';
import { Logger } from '../utils/Logger';
import { FileService } from '../service/FileService';
import { Persist } from '../store/Persist';
import { GroupStore } from '../store/GroupStore';
import { HomePage } from '../views/HomePage';
import { CameraPage } from '../views/CameraPage';
import { GroupEditPage } from '../views/GroupEditPage';
import { PageEditPage } from '../views/PageEditPage';
import { ProgressPage } from '../views/ProgressPage';
import { ResultPage } from '../views/ResultPage';

@Entry
@ComponentV2
struct Index {
  @Local stack: NavPathStack = new NavPathStack();
  @Local store: GroupStore = GroupStore.get();
  @Local booted: boolean = false;

  aboutToAppear(): void {
    this.bootstrap();
  }

  private async bootstrap(): Promise<void> {
    const ctx = getContext(this) as common.UIAbilityContext;
    try {
      await DeviceId.init(ctx);
      await AppConfig.init(ctx);
      FileService.init(ctx);
      const raw = await Persist.init(ctx);
      this.store.init(raw);
    } catch (err) {
      Logger.error('Index', '初始化失败');
    } finally {
      this.booted = true;
    }
  }

  @Builder
  pageMap(name: string) {
    if (name === 'camera') {
      CameraPage({ stack: this.stack })
    } else if (name === 'groupEdit') {
      GroupEditPage({ stack: this.stack })
    } else if (name === 'pageEdit') {
      PageEditPage({ stack: this.stack })
    } else if (name === 'progress') {
      ProgressPage({ stack: this.stack })
    } else if (name === 'result') {
      ResultPage({ stack: this.stack })
    }
  }

  build() {
    Stack() {
      if (this.booted) {
        Navigation(this.stack) {
          HomePage({ stack: this.stack })
        }
        .hideTitleBar(true)
        .mode(NavigationMode.Stack)
        .navDestination(this.pageMap)
        .backgroundColor(Theme.PAPER)
      } else {
        Column() {
          LoadingProgress().width(40).height(40).color(Theme.ACCENT)
          Text('拾页')
            .fontSize(16)
            .fontColor(Theme.MUTED)
            .margin({ top: 16 })
        }
        .width('100%')
        .height('100%')
        .justifyContent(FlexAlign.Center)
        .backgroundColor(Theme.PAPER)
      }
    }
    .width('100%')
    .height('100%')
  }
}
