import { createHash, createPublicKey, verify } from 'node:crypto'
import { readFileSync } from 'node:fs'

// Same Minisign format and prehash as the Tauri updater. Never reads a private key.
export function verifyUpdater(bytes, encodedSignature, encodedPublicKey) {
  const publicLines = Buffer.from(encodedPublicKey.trim(), 'base64').toString('utf8').trim().split(/\r?\n/)
  const sigLines = Buffer.from(encodedSignature.trim(), 'base64').toString('utf8').trim().split(/\r?\n/)
  const key = Buffer.from(publicLines[1] ?? '', 'base64')
  const sig = Buffer.from(sigLines[1] ?? '', 'base64')
  const globalSignature = Buffer.from(sigLines[3] ?? '', 'base64')
  if (key.length !== 42 || sig.length !== 74 || globalSignature.length !== 64 || !sigLines[2]?.startsWith('trusted comment: ')) throw new Error('Invalid Minisign encoding')
  if (!key.subarray(2, 10).equals(sig.subarray(2, 10))) throw new Error('Updater key ID does not match configured public key')
  const algorithm = sig.subarray(0, 2).toString('ascii')
  if (!['Ed', 'ED'].includes(algorithm)) throw new Error('Unsupported Minisign algorithm')
  const publicKey = createPublicKey({ format: 'der', type: 'spki', key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]) })
  const payload = algorithm === 'ED' ? createHash('blake2b512').update(bytes).digest() : bytes
  const signature = sig.subarray(10)
  if (!verify(null, payload, publicKey, signature) || !verify(null, Buffer.concat([signature, Buffer.from(sigLines[2].slice(17))]), publicKey, globalSignature)) throw new Error('Invalid updater signature')
}

if (process.argv[1]?.endsWith('verify-updater-signature.mjs')) {
  const [artifact, signaturePath, configPath = 'src-tauri/tauri.conf.json'] = process.argv.slice(2)
  if (!artifact || !signaturePath) throw new Error('Usage: node tools/verify-updater-signature.mjs <artifact> <artifact.sig> [tauri.conf.json]')
  const bytes = readFileSync(artifact)
  const signature = readFileSync(signaturePath, 'utf8')
  const key = JSON.parse(readFileSync(configPath, 'utf8')).plugins.updater.pubkey
  verifyUpdater(bytes, signature, key)
  const damaged = Buffer.from(bytes); damaged[Math.floor(damaged.length / 2)] ^= 1
  let rejected = false
  try { verifyUpdater(damaged, signature, key) } catch { rejected = true }
  if (!rejected) throw new Error('Modified artifact was accepted')
  console.log('PASS: configured updater public key verifies artifact; modified artifact rejected')
}
