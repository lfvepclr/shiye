import { Theme } from '../common/Theme';

/** 空态：一叠纸 + 一句引导。 */
@ComponentV2
export struct EmptyState {
  @Param title: string = '还没有文档';
  @Param hint: string = '';

  build() {
    Column() {
      Stack() {
        ForEach([0, 1, 2], (index: number) => {
          Column()
            .width(72)
            .height(92)
            .backgroundColor(Theme.CARD)
            .borderRadius(6)
            .border({ width: 1, color: Theme.LINE })
            .shadow({ radius: 8, color: 'rgba(26,26,26,0.05)', offsetX: 0, offsetY: 4 })
            .translate({ x: index * 6 - 6, y: index * -4 })
            .rotate({ angle: index * 4 - 4 })
        })
      }
      .width(110)
      .height(120)

      Text(this.title)
        .fontSize(16)
        .fontColor(Theme.INK)
        .fontWeight(FontWeight.Medium)
        .margin({ top: 24 })

      if (this.hint.length > 0) {
        Text(this.hint)
          .fontSize(13)
          .fontColor(Theme.MUTED)
          .textAlign(TextAlign.Center)
          .margin({ top: 8 })
          .padding({ left: 40, right: 40 })
      }
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .layoutWeight(1)
  }
}
