const { main } = require("./approve-upgrade-multisig");

// New triumvirate (June 22, 2026). 2-of-3 derives sudo multisig
// 5DcSqBNqCmfdJZRGFSwwcRb2dZdJHZuKK8Tb1Gx8gbmF5E8s.
const SUDO_SIGNATORIES = [
  "5E7RCRrPVS8TckCDjr92B5ciGziwz2kfvxe4URy3L7AgirGJ", // A
  "5FevFjov8435t5XC2MUSRpFYxtthE8pZy1toHpgAAia3ZphG", // B
  "5GRCukV2rZmSVfJhAXoLjcrU1pMVCf2Ra1ydbiCFZdaQXDXo", // C
];
const SUDO_THRESHOLD = 2; // 2 of 3

main(SUDO_SIGNATORIES, SUDO_THRESHOLD).catch((error) => {
  console.error(error.stack);
  process.exit(1);
});
