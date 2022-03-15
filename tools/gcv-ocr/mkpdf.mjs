import PDFDocument from 'pdfkit'
import sizeOfCallback from 'image-size'
import * as fs from 'fs/promises'
import { createWriteStream } from 'fs'
import { promisify } from 'util'
import * as zlib from 'zlib'

const sizeOf = promisify(sizeOfCallback)

async function main() {
	const ocrFiles = (await fs.readdir('.'))
		.filter(f => f.endsWith('.png.json'))
		.filter(f => {
			const n = parseInt(f.substring('Image ('.length))
			return n >= 1 && n <= 8
		})
	ocrFiles.sort((a, b) => a.localeCompare(b, undefined, {
		numeric: true,
		sensitivity: 'base',
	}))
	const doc = new PDFDocument({
		autoFirstPage: false,
		size: 'A4',
	})
	for (const ocrFile of ocrFiles) {
		doc.addPage({ margin: 0 })
		const tmp = '../../scans/' + ocrFile.substring(0, ocrFile.length - '.png.json'.length)
		const imageSize = await sizeOf(tmp + '.png')
		doc.image(tmp + '.jpg', 0, 0, { width: doc.page.width, height: doc.page.height })
		const scaleX = doc.page.width / imageSize.width
		const scaleY = doc.page.height / imageSize.height
		const ocrJson = JSON.parse(await fs.readFile(ocrFile, 'utf-8'))
		for (let page of ocrJson.fullTextAnnotation.pages) {
			for (let block of page.blocks) {
				for (let paragraph of block.paragraphs) {
					for (let word of paragraph.words) {
						if (word.confidence < 0.6) {
							continue
						}
						const text = word.symbols.map(s => s.text).join('')
						const bbox = word.boundingBox.vertices
						doc.save()
						doc.fontSize(scaleY * (bbox[2].y - bbox[1].y))
						const originalWidth = doc.widthOfString(text)
						if (originalWidth === 0) {
							continue
						}
						const x = scaleX * bbox[0].x
						const y = scaleY * bbox[0].y
						doc.scale(scaleX * (bbox[1].x - bbox[0].x) / originalWidth, 1, { origin: [x, y] })
						doc.opacity(0)
						doc.text(text, x, y, { lineBreak: false })
						doc.restore()
					}
				}
			}
		}
	}
	doc.pipe(createWriteStream('out.pdf'))
	doc.end()
}

main()
