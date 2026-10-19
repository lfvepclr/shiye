import { Theme } from '../common/Theme';

/** 状态徽标：未生成 / 上传中 / 识别中 n/N / 已完成 / 失败。 */
@ComponentV2
export struct StatusChip {
  @Param label: string = '';
  @Param color: string = Theme.MUTED;

  build() {
    Text(this.label)
      .fontSize(11)
      .fontColor(this.color)
      .backgroundColor(this.color + '1A')
      .borderRadius(999)
      .padding({ left: 8, right: 8, top: 3, bottom: 3 })
  }
}
