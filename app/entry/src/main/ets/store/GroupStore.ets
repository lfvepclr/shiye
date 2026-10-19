import { DocGroup, GroupJson, groupFromJson, groupToJson } from '../model/DocGroup';
import { DocPage } from '../model/DocPage';
import { Persist } from './Persist';
import { Logger } from '../utils/Logger';
import { Time } from '../utils/Time';

/** 全局唯一的组状态源。所有页面只改这里，再由这里落盘。 */
@ObservedV2
export class GroupStore {
  private static inst: GroupStore | null = null;

  @Trace groups: DocGroup[] = [];
  @Trace currentGroupId: string = '';
  @Trace currentPageId: string = '';
  @Trace ready: boolean = false;

  static get(): GroupStore {
    if (GroupStore.inst === null) {
      GroupStore.inst = new GroupStore();
    }
    return GroupStore.inst;
  }

  init(raw: string): void {
    if (raw.length > 0) {
      try {
        const list = JSON.parse(raw) as GroupJson[];
        const groups: DocGroup[] = [];
        for (let i = 0; i < list.length; i++) {
          const g = groupFromJson(list[i]);
          g.syncOrd();
          groups.push(g);
        }
        this.groups = groups;
      } catch (err) {
        Logger.error('Store', '本地数据损坏，已忽略');
      }
    }
    this.ready = true;
  }

  add(group: DocGroup): void {
    this.groups = [group].concat(this.groups);
    this.currentGroupId = group.id;
    this.persist();
  }

  remove(groupId: string): void {
    const next: DocGroup[] = [];
    for (let i = 0; i < this.groups.length; i++) {
      if (this.groups[i].id !== groupId) {
        next.push(this.groups[i]);
      }
    }
    this.groups = next;
    if (this.currentGroupId === groupId) {
      this.currentGroupId = '';
    }
    this.persist();
  }

  find(groupId: string): DocGroup | undefined {
    for (let i = 0; i < this.groups.length; i++) {
      if (this.groups[i].id === groupId) {
        return this.groups[i];
      }
    }
    return undefined;
  }

  current(): DocGroup | undefined {
    if (this.currentGroupId.length === 0) {
      return undefined;
    }
    return this.find(this.currentGroupId);
  }

  setCurrent(groupId: string): void {
    this.currentGroupId = groupId;
  }

  newGroup(groupId: string, title: string): DocGroup {
    const group = new DocGroup(groupId);
    group.title = title.length > 0 ? title : Time.groupTitle(Time.now());
    group.createdAt = Time.now();
    group.updatedAt = group.createdAt;
    return group;
  }

  addPage(group: DocGroup, page: DocPage): void {
    page.ord = group.pages.length;
    group.pages = group.pages.concat([page]);
    group.updatedAt = Time.now();
    this.persist();
  }

  removePage(group: DocGroup, pageId: string): void {
    const next: DocPage[] = [];
    for (let i = 0; i < group.pages.length; i++) {
      if (group.pages[i].id !== pageId) {
        next.push(group.pages[i]);
      }
    }
    group.pages = next;
    group.syncOrd();
    group.updatedAt = Time.now();
    this.persist();
  }

  touch(group: DocGroup): void {
    group.updatedAt = Time.now();
    this.persist();
  }

  persist(): void {
    const list: GroupJson[] = [];
    for (let i = 0; i < this.groups.length; i++) {
      list.push(groupToJson(this.groups[i]));
    }
    Persist.schedule(JSON.stringify(list));
  }

  flush(): void {
    Persist.flush();
  }
}
