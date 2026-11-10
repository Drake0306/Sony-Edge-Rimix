# Testing Rimix with your real camera

Everything in Rimix so far has only been tried against a simulated camera. This guide is how we find out what your ZV-E10 (firmware 2.00) really does. Plan on about 30 minutes. Do the steps in order, top to bottom, and after each one send back what the "What to send back" line asks for.

Three steps decide what happens next. They are marked **Changes the plan**: step 2 (the QR test), step 3 (pairing) and step 4 (the property dump).

## Before you start

- Charge the battery. Put a card in the camera.
- Build the command line tool once, from the repository folder:

  ```
  cargo build --release
  ```

  The tool is then `target/release/rimix`. The commands below say `rimix`; either put that folder on your path or write `target/release/rimix` each time.
- If a menu name below does not match what you see on the camera, pick the closest one and write down the real name. That is useful information.

## Please read this first

- **Pairing is permanent.** In step 3 the camera will remember your computer until you reset the camera. This is the same thing Sony's own desktop app (Imaging Edge Desktop) does. It does not change your photos or settings.
- **Nothing in this guide sends unknown commands.** You only run the `rimix` commands and the Rimix app listed here. Please do not run anything else against the camera.
- If the camera ever freezes, see "If something goes wrong" at the end.

## 1. Set up the camera

1. Menu, Network, "Ctrl w/ Smartphone": set it to Off. (Sony's phone app and this computer cannot share the camera.)
2. Menu, Network, "PC Remote Function": set "PC Remote" to On.
3. Under the same menu, set "PC Remote Cnct Method" to Wi-Fi Direct.
4. Open "Wi-Fi Direct Info." The camera shows a network name (it starts with DIRECT-) and a password. Write both down, or take a photo.

Later, for a test on your home router instead of Wi-Fi Direct, set "PC Remote Cnct Method" to Access Point and join the camera to your router. You do not need that now.

**What to send back:** a photo of the Wi-Fi Direct Info screen if you like (it is fine to blank the password), plus the real names of any menu items that differed from the ones above.

## 2. The QR test (Changes the plan)

This tells us whether the QR code on the camera leads to the same network the computer control uses, and whether Rimix can join that network by itself.

1. On the camera: Menu, Network, "Send to Smartphone Func.", "Send to Smartphone". A QR code appears.
2. Start the Rimix app (step 7 shows how). On the connect screen, use the "Scan the QR code" card and hold the camera's screen up to the Mac's webcam. Note what the message on the card says, and what the status line says while Rimix works. Check whether the Mac joined the DIRECT-xxxx network by itself.
3. Now scan the same QR code with your phone's camera app. Do not install anything. The phone will show some text, possibly a network name and password. Copy that text exactly, and paste it into "Paste the code's text" under the QR card in Rimix. Note the status line again.
4. If the Mac did not join by itself, join it yourself to the DIRECT-xxxx name from step 1 (the password is the one from Wi-Fi Direct Info).
5. In Terminal, run:

   ```
   nc -vz 192.168.122.1 15740
   ```

   This checks whether the camera answers on the port Rimix uses. "succeeded" or "open" means yes. "refused" or a timeout means no. If it says no, put the camera on the PC Remote screen (step 1) and try once more.

**What to send back:** the text the phone decoded (blank the password if you prefer, but say whether it matched the Wi-Fi Direct Info screen), what Rimix's status line said, whether the Mac joined the DIRECT-xxxx network by itself, and the output of `nc -vz 192.168.122.1 15740`. Say plainly: "the port answered" or "the port did not answer".

## 3. Pairing (Changes the plan)

Keep the Mac on the camera's DIRECT-xxxx network and the camera on its PC Remote screen. Run:

```
rimix connect 192.168.122.1
```

The camera screen should show the name of this computer (add `--name "Roy's Mac"` if you want a different name) and ask you to allow the connection. Press OK on the camera. The tool then prints the camera's model, firmware and battery, and exits.

If it fails, run it again with `-v` for more detail:

```
rimix connect 192.168.122.1 -v
```

**What to send back:** the whole terminal output (copy and paste it, successful or not), and a photo of the camera screen at the moment it asked you to confirm. If the camera showed nothing at all, say so.

## 4. The property dump (Changes the plan)

This reads every setting the camera reports. It is read-only. We use it to correct the simulator, so it needs to be the real thing.

```
rimix props 192.168.122.1 --all > zv-e10-props.txt
```

**What to send back:** the file `zv-e10-props.txt` (it is created in the folder you ran the command from). If the command printed an error instead, paste that.

## 5. Take a picture

```
rimix shoot 192.168.122.1 --out ~/Desktop/rimix-test
```

The camera should take a picture, and a photo file should appear in `~/Desktop/rimix-test`.

**What to send back:** the terminal output and the name and size of the file (for example `ls -l ~/Desktop/rimix-test`). Say whether the camera actually fired and whether the file opens.

## 6. Live view

```
rimix liveview 192.168.122.1 --frames 30 --out ~/Desktop/rimix-test
```

This saves 30 live view frames as JPEG files.

**What to send back:** the terminal output, how many files appeared, and one of the images attached. Say whether the pictures looked smooth, flickered or were black.

## 7. The app

1. Start the app from the repository folder:

   ```
   cd apps/desktop
   pnpm install
   pnpm tauri dev
   ```

2. Enter `192.168.122.1` and press Connect. The Mac must still be on the camera's network.
3. Check that live view shows.
4. Press Shoot. Check a picture arrives in the filmstrip.
5. Change ISO using the ISO chip in the dock.
6. Press Record, wait a few seconds, press it again to stop. Check the camera has a new clip.

**What to send back:** a screenshot of the app with live view, and for each of connect, live view, shoot, ISO and record, one word: worked, did not work, or odd (with a sentence on what you saw). Paste any error text the app showed.

## If something goes wrong

- **The camera is frozen or stops answering.** Turn it off and on. If that does nothing, take the battery out for a few seconds. Then start again from step 1.
- **"Camera refused" (exit code 4).** Check that "Ctrl w/ Smartphone" is Off and that no phone is connected to the camera. Also check that PC Remote is On and the camera is on the PC Remote screen.
- **Connection timeout (exit code 3).** Check that the Mac is joined to the DIRECT-xxxx network (and not your home Wi-Fi). Then run `ping 192.168.122.1`. If ping gets no reply, the Mac is not on the camera's network.
- **Other exit codes.** 2 means a typing mistake in the command; 5 means a problem saving a file on the Mac (check the `--out` folder).
- **Rimix's saved files.** Rimix keeps its settings and the computer identity it shows to the camera in `~/Library/Application Support/Rimix/` (the `guid` file is the identity, `settings.json` is the app settings). The command line tool and the app share the same identity so the camera sees one computer.
  - Do not delete the `guid` file unless asked. If you do, the camera will see a new computer and ask to pair again, and the old pairing stays on the camera until you reset it.
  - Delete `settings.json` if the app behaves oddly after a settings problem. It is recreated on the next start.
