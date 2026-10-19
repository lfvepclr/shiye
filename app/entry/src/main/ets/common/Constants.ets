import { common } from '@kit.AbilityKit';
import { util } from '@kit.ArkTS';
import { Logger } from '../utils/Logger';

interface ClientConfig {
  serverBase: string;
  demoMode?: boolean;
}

/** 本地兜底：没放 config.json 时也能连本机服务端调试。 */
const FALLBACK_BASE = 'http://127.0.0.1:8080/shiye';

/** 运行期配置：服务器地址只从 rawfile 读，不进源码。 */
export class AppConfig {
  static serverBase: string = FALLBACK_BASE;
  static loaded: boolean = false;
  /** 调试开关：仅当 rawfile/config.json 显式打开时才出现「演示页」入口。 */
  static demoMode: boolean = false;

  static async init(ctx: common.UIAbilityContext): Promise<void> {
    const names: string[] = ['config.json', 'config.example.json'];
    for (const name of names) {
      try {
        const bytes = await ctx.resourceManager.getRawFileContent(name);
        const decoder = util.TextDecoder.create('utf-8');
        const text = decoder.decodeToString(bytes);
        const cfg = JSON.parse(text) as ClientConfig;
        const base = cfg.serverBase;
        // 示例文件里是 <HOST> 占位，不能直接当真实地址用
        if (base.length > 0 && base.indexOf('<') < 0) {
          AppConfig.serverBase = base.endsWith('/') ? base.substring(0, base.length - 1) : base;
        }
        AppConfig.demoMode = cfg.demoMode === true;
        AppConfig.loaded = true;
        Logger.info('Config', `serverBase=${AppConfig.serverBase} (${name})`);
        return;
      } catch (err) {
        Logger.warn('Config', `读取 ${name} 失败，尝试下一个`);
      }
    }
    AppConfig.loaded = true;
    Logger.warn('Config', `未找到配置，使用兜底地址 ${AppConfig.serverBase}`);
  }
}

/** 业务常量。 */
export class Constants {
  static readonly APP_NAME: string = '拾页';

  /** 上传用的正式边长（长边）。 */
  static readonly EXPORT_EDGE: number = 2000;
  /** 单页编辑实时预览边长。 */
  static readonly PREVIEW_EDGE: number = 640;
  /** 缩略图边长。 */
  static readonly THUMB_EDGE: number = 320;
  /** 连拍在途上限。 */
  static readonly INFLIGHT_MAX: number = 2;

  static readonly UPLOAD_CONCURRENCY: number = 2;
  static readonly UPLOAD_RETRY: number = 3;

  static readonly PREF_STORE: string = 'shiye_store';
  static readonly PREF_DEVICE: string = 'device_id';
  static readonly PREF_GROUPS: string = 'groups';
}
