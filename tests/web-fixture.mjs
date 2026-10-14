import {createEditorServer} from '../scripts/server.mjs';
const server=createEditorServer({port:Number(process.argv[2]),token:'browser-test-token',dataDir:process.argv[3],launch:async()=>{}});
server.listen(Number(process.argv[2]),'127.0.0.1',()=>console.log('ready'));
