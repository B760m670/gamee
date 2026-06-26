const { hairlineWidth } = require('nativewind/theme');

module.exports = {
  content: ['./app/**/*.{js,ts,jsx,tsx}', './components/**/*.{js,ts,jsx,tsx}'],
  presets: [require('nativewind/preset')],
  theme: {
    extend: {
      borderWidth: { hairline: hairlineWidth() },
    },
  },
  plugins: [],
};
