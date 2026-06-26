const { getDefaultConfig } = require('expo/metro-config');
const path = require('path');

const projectRoot = __dirname;
const monorepoRoot = path.resolve(projectRoot, '../..');

// Must be set before getDefaultConfig so the inline-env transform plugin
// can statically replace require.context(process.env.EXPO_ROUTER_APP_ROOT)
// in expo-router/_ctx.*.js. Without this, Metro receives `undefined` as the
// require.context path and the bundle fails.
if (!process.env.EXPO_ROUTER_APP_ROOT) {
  process.env.EXPO_ROUTER_APP_ROOT = path.resolve(projectRoot, 'app');
}

const config = getDefaultConfig(projectRoot);

// Monorepo: watch all workspace packages and resolve from both local and root
// node_modules so shared packages (packages/*) are found during bundling.
config.watchFolders = [...(config.watchFolders ?? []), monorepoRoot];
config.resolver.nodeModulesPaths = [
  path.resolve(projectRoot, 'node_modules'),
  path.resolve(monorepoRoot, 'node_modules'),
];

const originalResolveRequest = config.resolver.resolveRequest;
config.resolver.resolveRequest = (context, moduleName, platform) => {
  // Hermes can't compile dynamic import(variable) from OpenTelemetry
  if (moduleName.startsWith('@opentelemetry/')) {
    return { type: 'empty' };
  }
  // ws and stream are Node.js-only — shim for React Native
  if (moduleName === 'ws') {
    return { type: 'sourceFile', filePath: path.resolve(projectRoot, 'shims/ws.js') };
  }
  if (moduleName === 'stream' || moduleName === 'node:stream') {
    return { type: 'empty' };
  }
  if (originalResolveRequest) {
    return originalResolveRequest(context, moduleName, platform);
  }
  return context.resolveRequest(context, moduleName, platform);
};

module.exports = config;
