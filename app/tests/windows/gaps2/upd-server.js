// Local update server for the Windows updater test (throwaway key, no real endpoint).
//   node upd-server.js <dir> <port>
// <dir>/manifest.json   is served at /latest.json (rewritten between scenarios)
// <dir>/files/<name>    is served at /files/<name>
// <dir>/hold            while this file exists, a /latest.json request is held (the app checks once, at startup)
// <dir>/requests.log    one line per request: time, path, status, bytes
const http = require('http'), fs = require('fs'), path = require('path');
const dir = process.argv[2], port = Number(process.argv[3]);
const log = (s) => fs.appendFileSync(path.join(dir, 'requests.log'), `${new Date().toISOString()} ${s}\n`);
http.createServer(async (req, res) => {
  const p = req.url.split('?')[0];
  try {
    if (p === '/latest.json') {
      while (fs.existsSync(path.join(dir, 'hold'))) await new Promise(r => setTimeout(r, 100));
      let body = fs.readFileSync(path.join(dir, 'manifest.json'));
      if (body[0] === 0xEF && body[1] === 0xBB && body[2] === 0xBF) body = body.subarray(3); // PowerShell 5's UTF8 adds a BOM
      res.writeHead(200, { 'content-type': 'application/json', 'content-length': body.length }); res.end(body);
      log(`${p} 200 ${body.length}`);
    } else if (p.startsWith('/files/') && !p.includes('..')) {
      const body = fs.readFileSync(path.join(dir, 'files', path.basename(p)));
      res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': body.length }); res.end(body);
      log(`${p} 200 ${body.length}`);
    } else { res.writeHead(404); res.end(); log(`${p} 404 0`); }
  } catch (e) { res.writeHead(404); res.end(); log(`${p} 404 ${e.code || e}`); }
}).listen(port, '127.0.0.1', () => log(`listening ${port}`));
