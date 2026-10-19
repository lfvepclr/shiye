import { DocGroup, GroupState } from '../model/DocGroup';
import { DocPage, PageStatus } from '../model/DocPage';
import { GroupStore } from '../store/GroupStore';
import { ApiClient, ApiError, UploadPageDto } from './ApiClient';
import { FileService } from './FileService';
import { Constants } from '../common/Constants';
import { Logger } from '../utils/Logger';

class Counter {
  value: number = 0;
}

function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve: () => void) => {
    setTimeout(resolve, ms);
  });
}

/** 上传队列：每页一个请求，并发 2，失败只重传该页。 */
export class UploadQueue {
  /** 上传全部未上传页 → 同步页序 → 触发识别。 */
  static async run(group: DocGroup,
    onProgress: (done: number, total: number) => void): Promise<void> {
    const store = GroupStore.get();
    const pending: DocPage[] = [];
    for (let i = 0; i < group.pages.length; i++) {
      if (!group.pages[i].uploaded) {
        pending.push(group.pages[i]);
      }
    }

    group.state = GroupState.UPLOADING;
    group.error = '';
    group.doneCount = 0;
    store.touch(group);

    const total = pending.length;
    const counter = new Counter();
    onProgress(0, total);

    const workers: Promise<void>[] = [];
    const lanes = Math.min(Constants.UPLOAD_CONCURRENCY, total);
    for (let i = 0; i < lanes; i++) {
      workers.push(UploadQueue.lane(pending, group, counter, onProgress, total));
    }
    await Promise.all(workers);

    let uploadFailed = 0;
    for (let i = 0; i < group.pages.length; i++) {
      const page = group.pages[i];
      if (page.status === PageStatus.FAILED) {
        uploadFailed = uploadFailed + 1;
      }
    }
    if (uploadFailed > 0) {
      group.state = GroupState.FAILED;
      group.error = `${uploadFailed} 页上传失败，点重试`;
      store.touch(group);
      throw new Error(group.error);
    }

    const order = group.remotePageIds();
    if (order.length > 0) {
      await ApiClient.patchGroup(group.id, '', order);
    }

    await ApiClient.render(group.id);
    group.state = GroupState.RECOGNIZING;
    group.updatedAt = Date.now();
    store.touch(group);
  }

  private static async lane(queue: DocPage[], group: DocGroup, counter: Counter,
    onProgress: (done: number, total: number) => void, total: number): Promise<void> {
    while (queue.length > 0) {
      const page = queue.shift();
      if (page === undefined) {
        return;
      }
      await UploadQueue.uploadOne(group, page);
      counter.value = counter.value + 1;
      onProgress(counter.value, total);
    }
  }

  private static async uploadOne(group: DocGroup, page: DocPage): Promise<void> {
    page.status = PageStatus.UPLOADING;
    const source = page.workPath.length > 0 ? page.workPath : page.originPath;
    const bytes = FileService.read(source);

    const replacing = page.remoteId.length > 0;
    let attempt = 0;
    let dto: UploadPageDto | undefined = undefined;
    let lastMessage = '';
    while (attempt < Constants.UPLOAD_RETRY) {
      try {
        dto = replacing
          ? await ApiClient.replacePage(page.remoteId, bytes)
          : await ApiClient.uploadPage(group.id, bytes);
        break;
      } catch (err) {
        const apiError = err as ApiError;
        lastMessage = apiError.message;
        // 4xx 是内容问题，重试没意义
        if (apiError.code >= 400 && apiError.code < 500) {
          break;
        }
        attempt = attempt + 1;
        if (attempt < Constants.UPLOAD_RETRY) {
          await sleep(attempt * 800);
        }
      }
    }

    if (dto === undefined) {
      page.status = PageStatus.FAILED;
      page.error = lastMessage.length > 0 ? lastMessage : '上传失败';
      Logger.error('Upload', `页面 ${page.id} 上传失败: ${page.error}`);
      return;
    }

    page.remoteId = dto.page_id;
    page.status = PageStatus.PENDING;
    page.uploaded = true;
    page.w = dto.w;
    page.h = dto.h;
    page.error = '';
  }
}
