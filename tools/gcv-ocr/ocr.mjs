import * as vision from '@google-cloud/vision'
import * as fs from 'fs/promises'
import { constants as fsConstants } from 'fs'

async function main () {
	const input = '../../scans/'
	const files = (await Promise.all((await fs.readdir(input))
		.filter(f => f.endsWith('.png'))
		.map(async f => {
			try {
				await fs.access(f + '.json', fsConstants.F_OK)
				return undefined
			} catch (err) {
				if (err.code !== 'ENOENT') {
					throw err
				}
				return f
			}
		})))
		.filter(f => f !== undefined)
	files.sort((a, b) => a.localeCompare(b, undefined, {
		numeric: true,
		sensitivity: 'base',
	}))
	const bytes = await Promise.all(files.map(f => fs.readFile(input + f)))
	const client = new vision.ImageAnnotatorClient()
	const chunkSize = 2
	for (let i = 0; i < files.length; i += chunkSize) {
		const chunk = bytes.slice(i, i + chunkSize)
		const requests = chunk.map(b => {
			return {
				image: { content: b.toString('base64') },
				features: [{
					type: 'DOCUMENT_TEXT_DETECTION',
					model: 'builtin/latest',
				}],
				imageContext: {
					languageHints: ['fr-t-i0-handwrit']
				}
			}
		})
		console.log('Sending request...')
		const [result] = await client.batchAnnotateImages({ requests })
		for (let j = 0; j < result.responses.length; j++) {
			let name = files[i + j]
			await fs.writeFile(name + '.json', JSON.stringify(result.responses[j]))
		}
	}
}

main()
