const http = require('http');
const fs = require('fs');
const net = require('net');

const PROXY_PORT = parseInt(process.env.PROXY_PORT || '8082');
const METRO_PORT = parseInt(process.env.METRO_PORT || '8081');

function getBorePort() {
  try {
    const log = fs.readFileSync('/tmp/bore.log', 'utf8');
    const m = log.match(/bore\.pub:(\d+)/);
    return m ? m[1] : null;
  } catch { return null; }
}

function rewrite(text, borePort) {
  // Rewrite any reference to Metro's port to the bore tunnel port
  const re = (host) => new RegExp(host + ':' + METRO_PORT, 'g');
  const replacement = 'bore.pub:' + borePort;
  return text
    .replace(re('bore\\.pub'), replacement)
    .replace(re('localhost'), replacement)
    .replace(re('127\\.0\\.0\\.1'), replacement)
    .replace(re('10\\.0\\.\\d+\\.\\d+'), replacement);
}

const server = http.createServer((req, res) => {
  const borePort = getBorePort();
  const reqData = [];
  req.on('data', c => reqData.push(c));
  req.on('end', () => {
    const proxyReq = http.request({
      hostname: 'localhost',
      port: METRO_PORT,
      path: req.url,
      method: req.method,
      headers: { ...req.headers, host: `localhost:${METRO_PORT}`, 'accept-encoding': 'identity' }
    }, (proxyRes) => {
      const ct = proxyRes.headers['content-type'] || '';
      if (/json|javascript|text/.test(ct) && borePort) {
        const chunks = [];
        proxyRes.on('data', c => chunks.push(c));
        proxyRes.on('end', () => {
          let text = rewrite(Buffer.concat(chunks).toString(), borePort);
          const hdrs = { ...proxyRes.headers };
          delete hdrs['content-length'];
          hdrs['content-length'] = Buffer.byteLength(text);
          res.writeHead(proxyRes.statusCode, hdrs);
          res.end(text);
        });
      } else {
        res.writeHead(proxyRes.statusCode, proxyRes.headers);
        proxyRes.pipe(res);
      }
    });
    proxyReq.on('error', () => { res.writeHead(502); res.end('Metro not available'); });
    if (reqData.length) proxyReq.end(Buffer.concat(reqData));
    else proxyReq.end();
  });
});

server.on('upgrade', (req, socket, head) => {
  const dst = net.connect(METRO_PORT, 'localhost');
  dst.on('connect', () => {
    const lines = [`${req.method} ${req.url} HTTP/1.1`, `Host: localhost:${METRO_PORT}`,
      ...Object.entries(req.headers).map(([k, v]) => `${k}: ${v}`), '', ''];
    dst.write(lines.join('\r\n'));
    dst.write(head);
  });
  socket.pipe(dst); dst.pipe(socket);
  socket.on('error', () => dst.destroy());
  dst.on('error', () => socket.destroy());
});

server.listen(PROXY_PORT, () => console.log(`Proxy :${PROXY_PORT} -> Metro :${METRO_PORT}`));
