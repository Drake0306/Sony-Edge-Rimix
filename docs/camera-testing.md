# Testing Rimix with your real camera

Everything in Rimix so far has only been tried against a simulated camera. This guide is how we find out what your ZV-E10 (firmware 2.00) really does. Plan on about 30 minutes for steps 1 to 8, and about 30 more for steps 9 to 11. Do the steps in order, top to bottom, and after each one send back what the "What to send back" line asks for.

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

Good to know: the camera's QR code holds only the short part of the network name (for example `YKE1`) and the model (`ZV-E10`). Rimix puts them together as `DIRECT-<name>:<model>`, for example `DIRECT-YKE1:ZV-E10`, which is the camera's real network name (confirmed on 2026-11-11). So if your phone shows `S:YKE1` and the Mac joins `DIRECT-YKE1:ZV-E10`, that is expected.

### If Rimix says it can't see the network

Rimix may say "This computer can't see DIRECT-xxxx yet". Then:

1. Check the camera is on its Wi-Fi Direct Info screen (Menu, Network, PC Remote Function, Wi-Fi Direct Info) and stay on it.
2. Wait ten seconds and try again.
3. If it still fails, run this in Terminal (it asks for nothing), with your camera's network name:

   ```
   rimix -v wifi join "DIRECT-xxxx:ZV-E10" 2>&1
   ```

   Only if the Mac has never joined that network, give the password too:

   ```
   printf '%s\n' 'the password' | rimix -v wifi join "DIRECT-xxxx:ZV-E10" --password-stdin 2>&1
   ```

   `-v` prints every Wi-Fi command Rimix runs and its answer, with the password hidden. At the end it prints how many times it tried, the network it ended up on and the address, or the error.

Send back all the output of that command (exit code 3 means the network was not visible, 4 means some other failure) and the newest file in `~/Library/Application Support/Rimix/logs`.

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

Good to know: the ZV-E10 answers each of the Sony connect steps with 8 bytes of data (confirmed on 2026-11-11). That is expected, and Rimix reads them and carries on. To see the bytes, run with `RUST_LOG=debug` in front of the command and look for `SDIO_Connect answered with data`.

**What to send back:** the whole terminal output (copy and paste it, successful or not), and a photo of the camera screen at the moment it asked you to confirm. If the camera showed nothing at all, say so.

## 4. The property dump (Changes the plan)

This reads every setting the camera reports. It is read-only. We use it to correct the simulator, so it needs to be the real thing.

```
rimix props 192.168.122.1 --all > zv-e10-props.txt
```

**What to send back:** the file `zv-e10-props.txt` (it is created in the folder you ran the command from). If the command printed an error instead, paste that.

Good to know: while connected, Rimix keeps the settings fresh by reading the whole list again a few times a second, the same way libgphoto2 does (operation 0x9209 with no parameters). Earlier versions asked for "only what changed" (0x9209 with a parameter) and read the new-picture flag on its own (0x9251); the ZV-E10 never answered one of those, so the app dropped and reconnected every few seconds. If the app still says "disconnected" over and over, run this with the Mac on the camera's network:

```
RUST_LOG=debug ~/Downloads/Rimix-0.1.0/rimix watch 192.168.122.1 --exit-after 20 2>&1 | tail -80
```

It logs every operation Rimix sends (`transaction op=0x…`), and an operation the camera never answers shows as `operation timed out` with its `op` named. Paste the output.

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

2. Open "Advanced: connect by address" on the connect screen, check the address says `192.168.122.1`, and press its Connect button. The Mac must still be on the camera's network.
3. Check that live view shows.
4. Press Shoot. Check a picture arrives in the filmstrip (Last captures) with a progress bar while it copies, and that the file is in the Save to folder. If the camera shoots RAW, check the RAW file arrived too.
5. Change ISO using the ISO chip in the dock: plus and minus, then type a value into Custom under it.
6. Look at the Mode chip. Turn the camera's mode dial through its positions and check each shows a name, not "Unknown (0x....)".
7. Press Record, wait a few seconds, press it again to stop. Check the camera has a new clip. Then watch Last captures: either the clip starts copying, or Rimix says "Videos stay on the card".
8. Turn on "Use as a webcam" in the Cameras panel. If OBS is installed, add a Browser source with the address shown and check the picture appears there.

**What to send back:** a screenshot of the app with live view, and for each of connect, live view, shoot, picture on the computer, ISO, mode names, record, video copy and webcam, one word: worked, did not work, or odd (with a sentence on what you saw). Also send the file `camera-<model>.json` from the log folder: Rimix writes it on every connect, and it lists every setting the camera reported. Paste any error text the app showed.

## 8. If your camera uses Access Authentication

Your ZV-E10 does not, so skip this step. It is for newer bodies (a7 IV, a7R V, a7CR, a7C II, a7 V, a6700, a9 III, a1 II, ZV-E1, ZV-E10 II, FX3, FX30; the a7 IV and FX3 from firmware 2.00). The camera shows its username, password and fingerprint under "Access Authen. Info". In Rimix, open "This camera uses Access Authentication" on the connect screen and type the Username and Password. Rimix then shows the camera's fingerprint in two forms (SHA256 and MD5); the camera may show either, so compare the one it shows and press "It matches, connect". If Rimix says the fingerprint changed, it shows the stored one and the camera's current one and will not connect: if you reset the camera, forget it in App settings and connect again. A lock appears in the top bar once connected.

**What to send back:** whether the fingerprint on screen matched, whether the lock appeared, and the newest file in `~/Library/Application Support/Rimix/logs`. Never send the password.

## 9. Browse card

This tells us whether the ZV-E10 lists its card while Rimix controls it in PC Remote mode.

1. Connect in the app as in step 7.
2. In the Cameras panel, press "Browse card". Note what the panel shows: a list of files, "No files on the card", "This camera needs its own transfer mode for card browsing; not available yet", or an error.
3. If files are listed, press "Download" on one photo and, if there is one, on a short video. The row should show progress and then "Saved", and the file should appear in the filmstrip and the Save to folder.
4. Note what the camera's own screen shows while the panel is open.
5. Disconnect the app, then try the same from the command line (the Mac still on the camera's network):

   ```
   rimix card list 192.168.122.1
   rimix card get 192.168.122.1 <handle or name from the list> --out ~/Desktop/rimix-test
   ```

6. Run the listing once more with logging on, so a listing that stops at the card's top folders or comes back empty can be told apart from a camera that refuses card operations:

   ```
   rimix -v card list 192.168.122.1 2>&1
   ```

**What to send back:** what the Browse card panel showed, whether Download worked for a photo and a video (and the file sizes), what the camera's screen showed, the output of both `card` commands, the whole output of `rimix -v card list 2>&1`, and the newest file in `~/Library/Application Support/Rimix/logs`.

## 10. Receive from camera (SFTP)

This tests the camera sending files to Rimix with its own FTP Transfer function. For this one the camera and the Mac must be on the same router (your home Wi-Fi), not the camera's Wi-Fi Direct network.

If your camera's Network menu has no FTP Transfer Func., note that and skip to step 11; `rimix sftp serve` can still be checked from another computer with `sftp -P 2222 rimix@<address>`.

1. Join the Mac to your home Wi-Fi.
2. In the Rimix app, open App settings and turn on "Receive from camera". The section shows an Address, a Username, a Password and a Fingerprint.
3. On the camera: Menu, Network, FTP Transfer Func., Server Setting. Choose SFTP and enter the Address (the part before the colon as the host, the number after it as the port), the Username and the Password from Rimix. Connect the camera to your home Wi-Fi if it asks.
4. If the camera shows a fingerprint when it first connects, compare it with the Fingerprint in Rimix.
5. Send one test photo from the camera's FTP Transfer menu. It should appear in the filmstrip, labelled "Received", and in the Save to folder.
6. If the app does not work, try the command line instead (stop the app first, it uses the same port):

   ```
   printf '%s\n' 'a password' | rimix sftp serve --dir ~/Desktop/rimix-test --password-stdin
   ```

   It prints the address, user and fingerprint to enter on the camera (with "a password" as the password), then a "received" line for each file. Press Ctrl-C to stop it.

Write down the real names of the camera menu items you went through; ours are a guess beyond "FTP Transfer Func." and "Server Setting".

**What to send back:** the menu names you saw, whether the camera accepted the server settings, whether it showed a fingerprint and whether it matched, whether the test photo arrived (and its name and size), any error the camera or Rimix showed, and the newest file in `~/Library/Application Support/Rimix/logs`. Never send the password.

## 11. Bluetooth

This tests finding the camera over Bluetooth, reading its Wi-Fi details that way, and the Bluetooth remote shutter. macOS may ask to let Terminal (or Rimix) use Bluetooth; allow it.

1. Turn the camera's Bluetooth on: Menu, Network, Bluetooth. Write down the real menu names.
2. In Terminal:

   ```
   rimix ble scan
   ```

   It lists the Sony cameras heard: name, model, pairing and address. If the camera offers a way to wait for pairing in that menu, run the scan once while it waits and once without, and note the pairing column each time.
3. Read the camera's Wi-Fi details over Bluetooth, using the name or address from the scan:

   ```
   rimix ble handoff ZV-E10
   ```

   It prints `ssid:` with the network name and `password: (hidden, N chars)`. Compare the name with Wi-Fi Direct Info.
4. Pair the Mac from the camera's Bluetooth menu (Menu, Network, Bluetooth), accepting on both the camera and the Mac. Then:

   ```
   rimix ble shoot ZV-E10
   ```

   It prints `shot:` and the camera's name. Check whether the camera actually fired.
5. In the app, with Bluetooth on in App settings, check whether the connect screen's Nearby card shows "ZV-E10 nearby", and try "Join via Bluetooth".

**What to send back:** the output of `ble scan` (both times), `ble handoff` and `ble shoot` (blank the network name if you like, but say whether it matched Wi-Fi Direct Info), whether the shutter fired, what Nearby showed and whether Join via Bluetooth connected, the real Bluetooth menu names, and the newest file in `~/Library/Application Support/Rimix/logs`.

## If something goes wrong

- **Always send the newest log.** Rimix keeps daily logs in `~/Library/Application Support/Rimix/logs`. In App settings, press Show log folder on the Logs row, and attach the newest file to every report.

- **The camera is frozen or stops answering.** Turn it off and on. If that does nothing, take the battery out for a few seconds. Then start again from step 1.
- **"Camera refused" (exit code 4).** Check that "Ctrl w/ Smartphone" is Off and that no phone is connected to the camera. Also check that PC Remote is On and the camera is on the PC Remote screen.
- **Connection timeout (exit code 3).** Check that the Mac is joined to the DIRECT-xxxx network (and not your home Wi-Fi). Then run `ping 192.168.122.1`. If ping gets no reply, the Mac is not on the camera's network.
- **Rimix says macOS is keeping it off the local network** (on the connect screen's Nearby list, or after "Joined DIRECT-xxxx"). macOS is refusing Rimix every connection to nearby devices, so the camera cannot answer whatever its state. Open System Settings, Privacy & Security, Local Network and turn Rimix on; if it is already on, turn it off and on again. Then quit and reopen Rimix. The log shows this as `the system refused the SSDP search`.
- **The app keeps saying "Reconnecting".** Rimix tries to rejoin the camera's Wi-Fi by itself from the second attempt. From attempt 5 the top bar says to rejoin it from the Wi-Fi menu: join the DIRECT-xxxx network again there, or press Disconnect and connect again.
- **Other exit codes.** 2 means a typing mistake in the command; 5 means a problem saving a file on the Mac (check the `--out` folder).
- **Rimix's saved files.** Rimix keeps its settings and the computer identity it shows to the camera in `~/Library/Application Support/Rimix/` (the `guid` file is the identity, `settings.json` is the app settings). The command line tool and the app share the same identity so the camera sees one computer.
  - Do not delete the `guid` file unless asked. If you do, the camera will see a new computer and ask to pair again, and the old pairing stays on the camera until you reset it.
  - Delete `settings.json` if the app behaves oddly after a settings problem. It is recreated on the next start.
