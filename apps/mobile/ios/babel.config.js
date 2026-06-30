const path = require('path');

// Compute the same relative path that expoRouterBabelPlugin would produce:
//   path.relative(dirname(expo-router/entry), apps/mobile/app)  →  '../../app'
// This path is what Metro's require.context resolves against _ctx.*.js, so it
// must be relative — not absolute — otherwise routes aren't found on device.
const appFolder = path.resolve(__dirname, 'app');
const routerEntry = require.resolve('expo-router/entry', { paths: [__dirname] });
const relativeAppRoot = path.relative(path.dirname(routerEntry), appFolder);

module.exports = function (api) {
  api.cache(true);
  return {
    presets: ['babel-preset-expo'],
    plugins: [
      // babel-preset-expo's expoRouterBabelPlugin relies on Metro caller data to
      // replace process.env.EXPO_ROUTER_APP_ROOT and EXPO_ROUTER_IMPORT_MODE in
      // expo-router/_ctx.*.js. Under EAS Update the caller chain is absent for
      // node_modules files, leaving those references unreplaced and causing Metro's
      // require.context static analysis to throw a SyntaxError. This plugin runs
      // before presets and provides the replacements unconditionally.
      function expoRouterEnvFallback({ types: t }) {
        return {
          visitor: {
            MemberExpression(nodePath) {
              if (!nodePath.get('object').matchesPattern('process.env')) return;
              const name = nodePath.node.property.name;
              if (name === 'EXPO_ROUTER_APP_ROOT') {
                nodePath.replaceWith(t.stringLiteral(relativeAppRoot));
              } else if (name === 'EXPO_ROUTER_IMPORT_MODE') {
                nodePath.replaceWith(t.stringLiteral('sync'));
              }
            },
          },
        };
      },
      // Reanimated 4 worklets transform — MUST be the last plugin in the list.
      'react-native-worklets/plugin',
    ],
  };
};
