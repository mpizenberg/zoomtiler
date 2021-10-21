# zoomtiler

Simple CLI program to generate deepzoom zoomable tiled images.
The input can either be a single image or multiple images of the same height, which are supposed to be consecutive horizontally.

```sh
# Create a zoomable image from
# a list of images making up a horizontal panorama
zoomtiler *.png
```

![figure](https://user-images.githubusercontent.com/2905865/138273565-a22e44b0-8780-4ecf-ad96-4dae020e04a3.png)

As of writing this, the main advantage of zoomtiler over another tool like [deepzoom.py][deepzoom-py] or [libvips][libvips] is that it works with multiple consecutive input images.
Meaning, if the huge input image you want to tile into a deepzoom format actually does not exists, but is a horizontal panorama composed many consecutive images, this tool can generate the deepzoom tiles without needing to actually generate the huge panorama first.
Another advantage is that it's a 0-dependency, portable executable that you can just download from the [latest release][release].

[deepzoom-py]: https://github.com/openzoom/deepzoom.py
[libvips]: https://www.libvips.org/
[release]: https://github.com/mpizenberg/zoomtiler/releases
