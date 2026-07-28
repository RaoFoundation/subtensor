const { main } = require("./approve-upgrade-multisig");

const SUDO_SIGNATORIES = [
  "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY", // ALICE
  "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty", // BOB
  //"5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y", // CHARLIE
];
const SUDO_THRESHOLD = 2; // 2 of 2

main(SUDO_SIGNATORIES, SUDO_THRESHOLD).catch((error) => {
  console.error(error.stack);
  process.exit(1);
});
