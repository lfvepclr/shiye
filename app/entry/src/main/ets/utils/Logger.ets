import { hilog } from '@kit.PerformanceAnalysisKit';

const DOMAIN = 0x5348;
const TAG = 'Shiye';

/** 统一日志出口：Release 只打 info 及以上。 */
export class Logger {
  static debug(scope: string, msg: string): void {
    hilog.debug(DOMAIN, TAG, '[%{public}s] %{public}s', scope, msg);
  }

  static info(scope: string, msg: string): void {
    hilog.info(DOMAIN, TAG, '[%{public}s] %{public}s', scope, msg);
  }

  static warn(scope: string, msg: string): void {
    hilog.warn(DOMAIN, TAG, '[%{public}s] %{public}s', scope, msg);
  }

  static error(scope: string, msg: string): void {
    hilog.error(DOMAIN, TAG, '[%{public}s] %{public}s', scope, msg);
  }
}
