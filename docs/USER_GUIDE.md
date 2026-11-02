# User guide

What you see and press once the firmware is installed ([INSTALL.md](INSTALL.md)).

## The buttons

| Position | Button | Does |
| --- | --- | --- |
| Bottom, left bar, left end | **Left** `<` | Back one page or item |
| Bottom, left bar, right end | **Right** `>` | Forward one page or item |
| Bottom, right bar, left end | **Back** `←` | Leave this screen |
| Bottom, right bar, right end | **Confirm** `○` | Open or choose |
| Left side key | **Up** | Same as Left |
| Right side key | **Down** | Same as Right |
| Power | | Sleep and wake |

These are not CrossPoint's functions: CrossPoint uses Back and Confirm on the left
pair. The bar at the foot of each screen shows only the keys that do something
there.

## Home

A **Continue reading** card with a progress bar, and four tiles: **Library**,
**Pictures**, **Receive**, **Device**. Left and Right move the highlight. Confirm
opens it. Back does a full refresh, which clears ghosting. The Receive tile shows when
the last Receive finished. Power puts the X3 to sleep.

Screens change with two quick refreshes; every sixth change is a full refresh that
flashes once, which keeps the screen clean.

## Device

**Wi-Fi** chooses the network Receive tries first, or adds one. **Storage** shows what
is on the card and can delete it all. **Clean screen** clears ghosting. **About this
device** shows the firmware, battery and clock. **Back to CrossPoint** returns to the
stock reader (you need a computer to come back).

The small key on the top edge, opposite Power, restarts the device. Use it if the
screen stops responding. It does not delete anything on the card.

## Library

Your articles and summaries, newest first. A bold title with a **New** mark is
unread. A bar and a percentage mean you have started it. **Read** means you
reached the end.

In the reader, Right turns to the next page and Left to the previous one. Confirm
does a full refresh. Back leaves, and the device remembers your place. Leave from the
last page and the article counts as read; opening it again starts from the top.

Page turns take about 0.4 s. The device clears the whole screen every few pages to
keep it crisp.

## Pictures

The last 20 pictures you sent, shown in four greys: Magic: The Gathering tokens and your
own images. Up and Down move through the list; Confirm shows one. On a picture, Left shows
an older one and Right a newer one, Confirm makes it the sleep screen, and Back leaves. A picture marked "show" appears on its own
when a Receive ends.

To send your own image, open the companion's web page, choose **Picture** and pick a
JPEG, PNG, WebP or GIF. The preview appears on its own; queue it when it looks right. Large
photos are made smaller before they are sent. Line art and comics look best; photos are
dithered to four greys.

To send a token, choose **Token** and search for its name. Only tokens are listed, closest
names first, each with its power and toughness, colour and set. A two-sided token shows both
faces; pick the side to show.

Every token and picture the X3 has received is kept on the companion, under **Saved tokens**
and **Saved pictures** on the page (the newest 50). Choose one and press **Send again** to
queue it at once. **Forget** removes it from the companion only.

## Receive

Choose the **Receive** tile. The device switches Wi-Fi on, asks your companion
what is waiting, downloads it to the card, then switches Wi-Fi off. A result page
lists what arrived and what was removed. Press any key to go back to Home.

A Receive takes about 4 s when nothing is waiting, plus about 5 s for each picture and
about 10 s for each long article. If it cannot finish it says why. You can press
Receive again. Nothing half-saved ever shows up in the library.

### What it keeps

| Kind | Kept | When full |
| --- | --- | --- |
| Pictures | the last 20 | the oldest goes; the one on screen stays |
| Articles | 32 | an article you have finished goes first, otherwise the oldest received |

Delete an article or token from the companion's web page and the device removes it at
the next Receive.

**Long articles:** the reader opens articles up to the companion's limit of about 250 KiB of
text. It reads them from the card a part at a time, so a page turn now and then takes a moment longer.

### Wi-Fi setup

The X3 has no keyboard, so you add a network from your phone.

1. Device → Wi-Fi → **Add a network**. The X3 starts a hotspot and shows a QR code, the
   hotspot name (`X3-setup-…`) and a password. It changes every time.
2. Scan the code with the phone camera and join. The phone may warn that there is no
   internet. That is expected.
3. In a browser open **192.168.4.1**. Pick your network from the list or type its name, enter
   the password and tap Save.
4. The X3 closes the hotspot and tries the new network. The result is on its screen.

The hotspot stops after 5 minutes, or when you press Back. The X3 keeps up to four
networks. A fifth replaces the oldest, and saving a name again replaces its password.
When more than one is saved, Receive joins the strongest one it can see, unless you chose
one in Device → Wi-Fi: then it tries that one first (the X3 checks it straight away).

It does not work with networks that need a login page (hotels, cafés) or with
WPA-Enterprise.

### Receive away from home

If `SERVER.CFG` has an `https://` address, Receive works on any Wi-Fi you have saved (a
phone hotspot, a café). At home it tries the LAN address first. Away from home it takes a
few seconds longer, because it sets up an encrypted connection. If Receive says
"The reply was not signed by your companion", the route is misconfigured or something is
tampering with the connection: do not trust it, and check
[COMPANION_SETUP.md](COMPANION_SETUP.md).

## Power

- The screen's power goes off after 3 s without a key press and comes back on the next
  press, on battery and on USB. (Leaving it on under a still picture for hours causes
  ghosting.)
- Press **Power** on Home to sleep now. On battery the device also sleeps after 10 minutes
  without a key press. The screen shows "Sleeping" with the article you were reading, or
  the picture you chose (open it in Pictures and press Confirm).
  Press **Power** to wake.
- On USB the device stands by instead of deep-sleeping (the chip cannot deep-sleep with a
  USB host). It looks the same; Power wakes it.
- If you see ghosting, choose **Device → Clean screen**: the screen flashes black and white
  for about 20 s.
- Wi-Fi is on only during a Receive. Reading is always offline.

## The files on the card

| File | What | Keep private? |
| --- | --- | --- |
| `X3/WIFI.CFG` | up to four saved networks, with passwords (Wi-Fi setup writes it) | yes |
| `X3/SERVER.CFG` | the companion's address and the device token | yes |
| `X3/DATA/` | articles and pictures (`.DAT`, `.MET`) | |
| `X3/LOG.TXT` | one line per Receive: time, counts, battery | |
| `X3/SHOWN.CFG` | a pending "displayed" note for the companion | |
| `X3/WIFI.PIN` | the network Receive tries first (name only) | |
| `X3/SLEEP.CFG` | the picture shown while asleep | |

The device writes to `DATA` in a way that survives a power cut: a half-written file
is ignored and cleaned up on the next Receive.

## When something goes wrong

| You see | Likely cause | Try |
| --- | --- | --- |
| "Cannot reach the companion" | server off, wrong address, or Wi-Fi up but no route | check `SERVER.CFG`; open the companion's page from your phone |
| "The companion refused the device token" | token wrong or removed | make a new one with `x3-companion device add NAME` and update `SERVER.CFG` |
| "Receive timed out" | weak Wi-Fi or a slow server | move closer; press Receive again |
| "Cannot write to the card" | card full, locked or damaged | check the card on a computer |
| "A download was damaged" | the network dropped bytes | press Receive again |
| Wi-Fi never joins right after a restart | the router still holds the old connection | wait a minute and try again |
| Library says "No articles" | nothing received yet | send one and choose Receive |

If a Receive ends with `Wi-Fi: …` and a code, the code is the router's reason. The
most common is a wrong password. Run Wi-Fi setup again and save the network once more.

## Sending things

You send articles and pictures from a phone or browser to the companion, not to the
device. Setting that up is in [COMPANION_SETUP.md](COMPANION_SETUP.md); the web page
and API are described in [PROTOCOL.md](PROTOCOL.md).
