import { expect, it } from 'vitest'
import { contentImages, contentInsertionRange } from './content-assets'
const id = '00000000-0000-7000-8000-000000000001'
it('带备注的本地图片仍可被关联，保留完整 Markdown，粘贴不能拆开标题', () => {
  const markdown = `![原图](lumen-asset:${id} "点击\\"发货\\"，备注")`
  expect(contentImages(markdown)).toEqual([{ id, name: '原图', markdown }])
  expect(contentInsertionRange(markdown, 20, 20)).toEqual({ start: markdown.length, end: markdown.length })
})
