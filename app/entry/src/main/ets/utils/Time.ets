/** 时间格式化：列表里只显示到分钟。 */
export class Time {
  private static pad(n: number): string {
    return n < 10 ? `0${n}` : `${n}`;
  }

  static now(): number {
    return Date.now();
  }

  static groupTitle(ts: number): string {
    const d = new Date(ts);
    return `${d.getFullYear()}-${Time.pad(d.getMonth() + 1)}-${Time.pad(d.getDate())} ` +
      `${Time.pad(d.getHours())}:${Time.pad(d.getMinutes())}`;
  }

  static human(ts: number): string {
    const now = Time.now();
    const diff = now - ts;
    const d = new Date(ts);
    const hhmm = `${Time.pad(d.getHours())}:${Time.pad(d.getMinutes())}`;
    const sameDay = new Date(now).toDateString() === d.toDateString();
    if (sameDay) {
      return `今天 ${hhmm}`;
    }
    const yesterday = new Date(now - 86400000);
    if (yesterday.toDateString() === d.toDateString()) {
      return `昨天 ${hhmm}`;
    }
    if (diff < 86400000 * 365) {
      return `${d.getMonth() + 1}月${d.getDate()}日 ${hhmm}`;
    }
    return Time.groupTitle(ts);
  }
}
