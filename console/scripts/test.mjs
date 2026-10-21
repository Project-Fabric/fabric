import { startVitest } from 'vitest/node';
import config from '../vitest.config.mjs';

// Load the configuration through Node. Tests still use Vitest's normal transforms and
// assertions; no native config bundling or parent-directory discovery is needed.
const context = await startVitest('test', [], { run: true, config: false }, { ...config, configFile: false });
if (!context) process.exitCode = 1;
else {
  if (!context.state.getFiles().length || context.state.getFiles().some(file => file.result?.state === 'fail') || context.state.getUnhandledErrors().length)
    process.exitCode = 1;
  await context.close();
}
