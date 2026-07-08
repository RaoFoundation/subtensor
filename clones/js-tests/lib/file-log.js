import fs from "node:fs";

const TEMP_DIR_URL = new URL("../temp/", import.meta.url);

export function createTempLogger(filename) {
  const fileUrl = new URL(filename, TEMP_DIR_URL);
  const buffer = [];
  let started = false;

  const append = (args) => {
    const line = `${args.map(formatValue).join(" ")}\n`;
    if (!started) {
      buffer.push(line);
      return Promise.resolve();
    }

    fs.appendFileSync(fileUrl, line);
    return Promise.resolve();
  };

  return {
    start() {
      if (started) {
        return Promise.resolve();
      }

      started = true;
      fs.mkdirSync(TEMP_DIR_URL, { recursive: true });
      fs.writeFileSync(fileUrl, buffer.join(""));
      buffer.length = 0;
      return Promise.resolve();
    },
    captureConsole() {
      console.log = (...args) => {
        void append(args);
      };
      console.error = (...args) => {
        void append(args);
      };
    },
    info(...args) {
      return append(args);
    },
    error(value) {
      return append([value]);
    },
    flush() {
      if (!started) {
        return this.start();
      }
      return Promise.resolve();
    },
  };
}

function formatValue(value) {
  if (value instanceof Error) {
    return value.stack ?? value.message;
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "bigint") {
    return value.toString();
  }
  return String(value);
}
