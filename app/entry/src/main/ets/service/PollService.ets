import { ApiClient, StatusDto } from './ApiClient';

/**
 * 进度轮询：1s 起 ×1.4 退避到 8s。
 * 服务端没有 SSE（`@ohos.net.http` 无 dataReceive），轮询是最稳的。
 */
export class PollService {
  private groupId: string = '';
  private timer: number = -1;
  private delay: number = 1000;
  private active: boolean = false;

  constructor(groupId: string) {
    this.groupId = groupId;
  }

  start(onUpdate: (status: StatusDto) => void, onFinish: (status: StatusDto) => void): void {
    this.active = true;
    this.delay = 1000;
    this.tick(onUpdate, onFinish);
  }

  private tick(onUpdate: (status: StatusDto) => void, onFinish: (status: StatusDto) => void): void {
    if (!this.active) {
      return;
    }
    ApiClient.status(this.groupId).then((status: StatusDto) => {
      if (!this.active) {
        return;
      }
      onUpdate(status);
      if (status.state === 'done' || status.state === 'failed') {
        this.active = false;
        onFinish(status);
        return;
      }
      this.scheduleNext(onUpdate, onFinish);
    }).catch(() => {
      this.scheduleNext(onUpdate, onFinish);
    });
  }

  private scheduleNext(onUpdate: (status: StatusDto) => void,
    onFinish: (status: StatusDto) => void): void {
    this.timer = setTimeout(() => {
      this.tick(onUpdate, onFinish);
    }, this.delay);
    this.delay = Math.min(8000, Math.round(this.delay * 1.4));
  }

  stop(): void {
    this.active = false;
    if (this.timer !== -1) {
      clearTimeout(this.timer);
      this.timer = -1;
    }
  }
}
