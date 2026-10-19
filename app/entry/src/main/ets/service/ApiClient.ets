import { http } from '@kit.NetworkKit';
import { util } from '@kit.ArkTS';
import { AppConfig } from '../common/Constants';
import { DeviceId } from '../utils/DeviceId';
import { Logger } from '../utils/Logger';

export class ApiError extends Error {
  code: number = 0;

  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
}

interface ErrorBody {
  error: string;
  message: string;
}

export interface GroupDto {
  id: string;
  title: string;
  state: string;
  page_count: number;
  done_count: number;
  docx_rev: number;
  created_at: number;
  updated_at: number;
}

export interface PageBriefDto {
  id: string;
  ord: number;
  status: string;
  w: number;
  h: number;
  error: string;
}

export interface GroupDetailDto {
  id: string;
  title: string;
  state: string;
  page_count: number;
  done_count: number;
  docx_rev: number;
  created_at: number;
  updated_at: number;
  pages: PageBriefDto[];
}

export interface UploadPageDto {
  page_id: string;
  ord: number;
  w: number;
  h: number;
}

export interface PageStatusDto {
  id: string;
  ord: number;
  status: string;
  w: number;
  h: number;
  error: string;
}

export interface StatusDto {
  state: string;
  done: number;
  total: number;
  docx_rev: number;
  error: string;
  pages: PageStatusDto[];
}

export interface ShareDto {
  token: string;
  url: string;
  expires_at: number;
}

interface CreateGroupBody {
  title: string;
}

interface PatchGroupBody {
  title?: string;
  page_order?: string[];
}

interface ShareBody {
  expires_in: number;
}

/** 服务端唯一出口。所有请求都带 `X-Device-Id`。 */
export class ApiClient {
  private static async send(path: string, method: http.RequestMethod,
    body: string | ArrayBuffer | undefined, contentType: string,
    binary: boolean): Promise<http.HttpResponse> {
    const req = http.createHttp();
    try {
      const header: Record<string, string> = {
        'X-Device-Id': DeviceId.get(),
        'Content-Type': contentType
      };
      const options: http.HttpRequestOptions = {
        method: method,
        header: header,
        expectDataType: binary ? http.HttpDataType.ARRAY_BUFFER : http.HttpDataType.STRING,
        connectTimeout: 15000,
        readTimeout: 180000
      };
      if (body !== undefined) {
        options.extraData = body;
      }
      const url = `${AppConfig.serverBase}${path}`;
      const resp = await req.request(url, options);
      if (resp.responseCode >= 400) {
        throw new ApiError(resp.responseCode, ApiClient.errorMessage(resp));
      }
      return resp;
    } catch (err) {
      if (err instanceof ApiError) {
        throw err;
      }
      const e = err as Error;
      Logger.error('Api', `${path} 失败: ${e.message}`);
      throw new ApiError(0, `网络请求失败: ${e.message}`);
    } finally {
      req.destroy();
    }
  }

  private static errorMessage(resp: http.HttpResponse): string {
    try {
      if (typeof resp.result === 'string') {
        const body = JSON.parse(resp.result as string) as ErrorBody;
        if (body.message !== undefined && body.message.length > 0) {
          return body.message;
        }
      }
    } catch (err) {
      // 非 JSON 错误体，落到状态码
    }
    return `HTTP ${resp.responseCode}`;
  }

  private static async json<T>(path: string, method: http.RequestMethod, body?: Object): Promise<T> {
    const text = body === undefined ? undefined : JSON.stringify(body);
    const resp = await ApiClient.send(path, method, text, 'application/json', false);
    return JSON.parse(resp.result as string) as T;
  }

  static async createGroup(title: string): Promise<GroupDto> {
    const body: CreateGroupBody = { title: title };
    return ApiClient.json<GroupDto>('/api/v1/groups', http.RequestMethod.POST, body);
  }

  static async listGroups(): Promise<GroupDto[]> {
    return ApiClient.json<GroupDto[]>('/api/v1/groups', http.RequestMethod.GET);
  }

  static async getGroup(groupId: string): Promise<GroupDetailDto> {
    return ApiClient.json<GroupDetailDto>(`/api/v1/groups/${groupId}`, http.RequestMethod.GET);
  }

  static async patchGroup(groupId: string, title: string, order: string[]): Promise<GroupDto> {
    const body: PatchGroupBody = {};
    if (title.length > 0) {
      body.title = title;
    }
    if (order.length > 0) {
      body.page_order = order;
    }
    // 注意：`http.RequestMethod` 没有 PATCH，传 PATCH 会退化成 GET（body 被拼进 query 导致 400），
    // 服务端 `/groups/{id}` 同时接受 PUT，语义与 PATCH 一致。
    return ApiClient.json<GroupDto>(`/api/v1/groups/${groupId}`, http.RequestMethod.PUT, body);
  }

  static async deleteGroup(groupId: string): Promise<void> {
    await ApiClient.send(`/api/v1/groups/${groupId}`, http.RequestMethod.DELETE, undefined, 'application/json', false);
  }

  static async deletePage(remotePageId: string): Promise<void> {
    await ApiClient.send(`/api/v1/pages/${remotePageId}`, http.RequestMethod.DELETE, undefined, 'application/json', false);
  }

  /** 手拼 multipart：ArkTS 没有 FormData。 */
  private static multipart(jpeg: ArrayBuffer, boundary: string): ArrayBuffer {
    const head = `--${boundary}\r\n` +
      'Content-Disposition: form-data; name="file"; filename="page.jpg"\r\n' +
      'Content-Type: image/jpeg\r\n\r\n';
    const tail = `\r\n--${boundary}--\r\n`;

    const encoder = util.TextEncoder.create('utf-8');
    const headBytes = encoder.encodeInto(head);
    const tailBytes = encoder.encodeInto(tail);
    const body = new Uint8Array(headBytes.length + jpeg.byteLength + tailBytes.length);
    body.set(headBytes, 0);
    body.set(new Uint8Array(jpeg), headBytes.length);
    body.set(tailBytes, headBytes.length + jpeg.byteLength);
    return body.buffer as ArrayBuffer;
  }

  static async uploadPage(groupId: string, jpeg: ArrayBuffer): Promise<UploadPageDto> {
    const boundary = `----shiye${Date.now()}`;
    const body = ApiClient.multipart(jpeg, boundary);
    const resp = await ApiClient.send(`/api/v1/groups/${groupId}/pages`,
      http.RequestMethod.POST, body,
      `multipart/form-data; boundary=${boundary}`, false);
    return JSON.parse(resp.result as string) as UploadPageDto;
  }

  /** 替换已有页的图片（重拍 / 重新编辑）。 */
  static async replacePage(pageId: string, jpeg: ArrayBuffer): Promise<UploadPageDto> {
    const boundary = `----shiye${Date.now()}`;
    const body = ApiClient.multipart(jpeg, boundary);
    const resp = await ApiClient.send(`/api/v1/pages/${pageId}`,
      http.RequestMethod.PUT, body,
      `multipart/form-data; boundary=${boundary}`, false);
    return JSON.parse(resp.result as string) as UploadPageDto;
  }

  static async render(groupId: string): Promise<void> {
    await ApiClient.send(`/api/v1/groups/${groupId}/render`, http.RequestMethod.POST, undefined, 'application/json', false);
  }

  static async status(groupId: string): Promise<StatusDto> {
    return ApiClient.json<StatusDto>(`/api/v1/groups/${groupId}/status`, http.RequestMethod.GET);
  }

  static async downloadDocx(groupId: string): Promise<ArrayBuffer> {
    const resp = await ApiClient.send(`/api/v1/groups/${groupId}/docx`, http.RequestMethod.GET, undefined, 'application/json', true);
    return resp.result as ArrayBuffer;
  }

  static async createShare(groupId: string, expiresIn: number): Promise<ShareDto> {
    const body: ShareBody = { expires_in: expiresIn };
    return ApiClient.json<ShareDto>(`/api/v1/groups/${groupId}/share`, http.RequestMethod.POST, body);
  }

  static async revokeShare(token: string): Promise<void> {
    await ApiClient.send(`/api/v1/shares/${token}`, http.RequestMethod.DELETE, undefined, 'application/json', false);
  }
}
