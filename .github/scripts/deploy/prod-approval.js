const { main } = require("./approve-upgrade-multisig");

// New triumvirate (June 22, 2026). 2-of-3 derives sudo multisig
// 5DcSqBNqCmfdJZRGFSwwcRb2dZdJHZuKK8Tb1Gx8gbmF5E8s. The signer set lives in
// sudo-signatories.json so the release-train manifest embeds the same list.
const {
  signatories: SUDO_SIGNATORIES,
  threshold: SUDO_THRESHOLD,
} = require("./sudo-signatories.json");

main(SUDO_SIGNATORIES, SUDO_THRESHOLD).catch((error) => {
  console.error(error.stack);
  process.exit(1);
});
