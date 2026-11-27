const readline = require('readline');

function askQuestion(rl, query, defaultValue) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      rl.removeListener('close', onClose);
      resolve(value);
    };
    const onClose = () => finish(defaultValue);
    rl.once('close', onClose);
    rl.question(query, (answer) => {
      finish(answer.trim() || defaultValue);
    });
  });
}

async function promptMenu() {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });

  const cyan = '\x1b[36m';
  const green = '\x1b[32m';
  const magenta = '\x1b[35m';
  const red = '\x1b[31m';
  const reset = '\x1b[0m';
  const bold = '\x1b[1m';

  try {
    console.log(`\n${bold}${magenta}╔══════════════════════════════════════════════════════════════╗${reset}`);
    console.log(`${bold}${magenta}║             Agent Guidance Setup Wizard                      ║${reset}`);
    console.log(`${bold}${magenta}╚══════════════════════════════════════════════════════════════╝${reset}\n`);

    console.log(`${bold}Select installation profile / action:${reset}`);
    console.log(`  ${green}[1] Full Standalone${reset}       (Local ML + SQLite FTS5 index)`);
    console.log(`  ${cyan}[2] Lightweight Client${reset}    (Zero-ML ~15MB RAM, connects to Remote Worker)`);
    console.log(`  ${magenta}[3] Dedicated Server Worker${reset} (ML worker node & real-time dashboard)`);
    console.log(`  ${red}[4] Uninstall${reset}              (Remove MCP server from all IDEs)`);
    console.log(`  [5] Help                   (Show CLI flags and options)\n`);

    const choice = (await askQuestion(rl, `${bold}Choice [1]: ${reset}`, '1')).toLowerCase();

    if (choice === '2' || choice === 'client') {
      const defaultUrl = 'http://127.0.0.1:11998';
      const serverUrl = await askQuestion(rl, `Remote Worker Server URL [${defaultUrl}]: `, defaultUrl);
      return [
        ['--set-server', serverUrl],
        ['--setup'],
      ];
    }

    if (choice === '3' || choice === 'server') {
      return [
        ['--setup-server'],
      ];
    }

    if (choice === '4' || choice === 'uninstall') {
      return [
        ['--uninstall'],
      ];
    }

    if (choice === '5' || choice === 'help') {
      return [
        ['--help'],
      ];
    }

    return [
      ['--setup'],
    ];
  } finally {
    rl.close();
  }
}

module.exports = { promptMenu, askQuestion };
