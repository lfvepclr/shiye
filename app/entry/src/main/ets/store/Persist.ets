import { common } from '@kit.AbilityKit';
import { preferences } from '@kit.ArkData';
import { Constants } from '../common/Constants';
import { Logger } from '../utils/Logger';

const DEBOUNCE_MS = 500;

/** 本地持久化：整份组列表写一个 key，500ms 防抖。 */
export class Persist {
  private static store: preferences.Preferences | null = null;
  private static timer: number = -1;
  private static pending: string = '';

  static async init(ctx: common.UIAbilityContext): Promise<string> {
    Persist.store = await preferences.getPreferences(ctx, Constants.PREF_STORE);
    const raw = Persist.store.getSync(Constants.PREF_GROUPS, '') as string;
    return raw;
  }

  static schedule(json: string): void {
    Persist.pending = json;
    if (Persist.timer !== -1) {
      clearTimeout(Persist.timer);
    }
    Persist.timer = setTimeout(() => {
      Persist.timer = -1;
      Persist.flush();
    }, DEBOUNCE_MS);
  }

  /** 立即写盘（退出页面时调用，避免防抖窗口内丢数据）。 */
  static flush(): void {
    const store = Persist.store;
    if (store === null || Persist.pending.length === 0) {
      return;
    }
    const payload = Persist.pending;
    Persist.pending = '';
    store.put(Constants.PREF_GROUPS, payload).then(() => {
      return store.flush();
    }).catch((err: Error) => {
      Logger.error('Persist', `写入失败: ${err.message}`);
    });
  }
}
