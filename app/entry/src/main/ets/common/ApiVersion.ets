import { deviceInfo } from '@kit.BasicServicesKit';

/** 系统能力门控：API 26 起才有沉浸光感材质。 */
export class ApiVersion {
  static api(): number {
    return deviceInfo.sdkApiVersion;
  }

  static supportsMaterial(): boolean {
    return deviceInfo.sdkApiVersion >= 26;
  }

  static supportsBlur(): boolean {
    return deviceInfo.sdkApiVersion >= 12;
  }
}
