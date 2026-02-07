const cwd = process.cwd()

const cssBuild = Bun.spawn(
  ['tailwindcss', '-i', './src/index.css', '-o', './src/styles.css', '--minify'],
  { cwd, stdout: 'inherit', stderr: 'inherit' },
)
const cssBuildCode = await cssBuild.exited
if (cssBuildCode !== 0) {
  process.exit(cssBuildCode)
}

const cssWatch = Bun.spawn(
  ['tailwindcss', '-i', './src/index.css', '-o', './src/styles.css', '--watch'],
  { cwd, stdout: 'inherit', stderr: 'inherit' },
)
const app = Bun.spawn(['bun', '--hot', '--port', '4173', 'index.html'], {
  cwd,
  stdout: 'inherit',
  stderr: 'inherit',
  stdin: 'inherit',
})

let shuttingDown = false
function shutdown(exitCode = 0) {
  if (shuttingDown) return
  shuttingDown = true
  cssWatch.kill()
  app.kill()
  process.exit(exitCode)
}

process.on('SIGINT', () => shutdown(0))
process.on('SIGTERM', () => shutdown(0))

const appCode = await app.exited
cssWatch.kill()
process.exit(appCode)
