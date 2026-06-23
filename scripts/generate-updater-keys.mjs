#!/usr/bin/env node
/**
 * Genera claves del updater en `.secrets/` y actualiza plugins.updater.pubkey.
 *
 * Uso: npm run updater:generate-keys -- "tu-contraseña"
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const secretsDir = join(root, ".secrets");
const keyPath = join(secretsDir, "nexosign.key");
const pubPath = `${keyPath}.pub`;
const tauriConfPath = join(root, "src-tauri", "tauri.conf.json");

const password = process.argv[2]?.trim();
if (!password) {
	console.error('Uso: npm run updater:generate-keys -- "tu-contraseña"');
	process.exit(1);
}

mkdirSync(secretsDir, { recursive: true });

const npmCmd = process.platform === "win32" ? "npm.cmd" : "npm";
const result = spawnSync(
	npmCmd,
	["run", "tauri", "signer", "generate", "--", "-w", keyPath, "-f", "-p", password],
	{ cwd: root, stdio: "inherit", shell: process.platform === "win32" },
);

if (result.status !== 0) {
	process.exit(result.status ?? 1);
}

const pubkey = readFileSync(pubPath, "utf8").trim();
const conf = JSON.parse(readFileSync(tauriConfPath, "utf8"));
conf.plugins.updater.pubkey = pubkey;
writeFileSync(tauriConfPath, `${JSON.stringify(conf, null, 2)}\n`, "utf8");

console.log("\nListo.");
console.log(`  Privada:  ${keyPath}`);
console.log(`  Pública:  ${pubPath}`);
console.log(`  Actualizado: ${tauriConfPath}`);
console.log("\nGitHub Secrets:");
console.log("  TAURI_SIGNING_PRIVATE_KEY          → .secrets/nexosign.key");
console.log("  TAURI_SIGNING_PRIVATE_KEY_PASSWORD → la misma contraseña del comando");
