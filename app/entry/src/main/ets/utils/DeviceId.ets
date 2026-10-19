import { common } from '@kit.AbilityKit';
import { preferences } from '@kit.ArkData';
import { util } from '@kit.ArkTS';
import { Constants } from '../common/Constants';
import { Logger } from './Logger';

/** 无账号体系：设备即归属，UUID 存 preferences。 */
export class DeviceId {
  private static value: string = '';

  static async init(ctx: common.UIAbilityContext): Promise<string> {
    if (DeviceId.value.length > 0) {
      return DeviceId.value;
    }
    const store = await preferences.getPreferences(ctx, Constants.PREF_STORE);
    const saved = store.getSync(Constants.PREF_DEVICE, '') as string;
    if (saved.length > 0) {
      DeviceId.value = saved;
    } else {
      DeviceId.value = util.generateRandomUUID(false);
      await store.put(Constants.PREF_DEVICE, DeviceId.value);
      await store.flush();
    }
    Logger.info('DeviceId', DeviceId.value);
    return DeviceId.value;
  }

  static get(): string {
    return DeviceId.value;
  }
}
