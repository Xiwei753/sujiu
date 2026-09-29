@Entry
@Component
struct Index {
  build() {
    Column({ space: 12 }) {
      Text('Sujiu')
        .fontSize(32)
        .fontWeight(FontWeight.Medium)

      Text('HarmonyOS shell · Rust core wiring comes next')
        .fontSize(16)
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .padding(24)
  }
}
