import grpc from '@grpc/grpc-js';
import protoLoader from '@grpc/proto-loader';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const protoPath = path.resolve('src-tauri/proto/app.proto');
const certPath = path.join(os.homedir(), 'Library/Application Support/com.splice.Splice/.certs/cert.pem');
const searchTerm = process.argv.slice(2).join(' ') || 'drum';

const def = protoLoader.loadSync(protoPath, {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
});
const pkg = grpc.loadPackageDefinition(def);
const App = pkg.proto.App;
const cert = fs.readFileSync(certPath);
const creds = grpc.credentials.createSsl(cert, null, null, { checkServerIdentity: () => undefined });

function call(client, method, request) {
  return new Promise((resolve, reject) => {
    client[method](request, (err, resp) => err ? reject(err) : resolve(resp));
  });
}

for (let port = 56765; port <= 56785; port++) {
  const client = new App(`127.0.0.1:${port}`, creds);
  try {
    const session = await call(client, 'GetSession', {});
    const login = await call(client, 'ValidateLogin', {});
    const prefs = await call(client, 'UserPreferences', {});
    const search = await call(client, 'SearchSamples', {
      SearchTerm: searchTerm,
      Purchased: 'All',
      MatchingTagsAndPacks: true,
      PerPage: 5,
      Page: 1,
    });

    console.log('connected', port, {
      hasAuth: !!session.Auth,
      hasToken: !!session.Auth?.Token,
      authType: session.Auth?.AuthType,
      username: login.User?.Username,
      credits: login.User?.Credits,
      spliceFolder: prefs.Preferences?.SpliceFolderPath,
      totalHits: search.TotalHits,
      samples: (search.Samples || []).map((s) => ({
        filename: s.Filename,
        fileHash: s.FileHash,
        purchased: Number(s.PurchasedAt || 0) > 0,
        localPath: s.LocalPath || undefined,
      })),
    });
    client.close();
    process.exit(0);
  } catch (e) {
    console.log(port, e.code, e.details || e.message);
    client.close();
  }
}
process.exit(1);
