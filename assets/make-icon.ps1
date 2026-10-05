# Regenerates wrecktangle.ico and wrecktangle-256.png. The design geometry
# must match wrecktangle.svg. Run with PowerShell 7 on Windows.

Add-Type -AssemblyName System.Drawing
# Newer .NET splits System.Drawing across internal assemblies that Add-Type
# only finds when given their paths.
$drawing = [System.Drawing.Bitmap].Assembly
$references = @($drawing.Location) + @($drawing.GetReferencedAssemblies() |
    Where-Object Name -Like 'System.*' |
    ForEach-Object { [System.Reflection.Assembly]::Load($_).Location })

Add-Type -ReferencedAssemblies $references -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;

public class LogoGeometry
{
    public float TileX, TileY, TileW, TileR;
    public float WinX, WinY, WinW, WinH, WinR, Stroke;
    public float BallX, BallY, BallR, GapR, RingR;
    public float CableX, CableY, CableW;
    public float HiX, HiY, HiR;

    // The 128-unit design from wrecktangle.svg, scaled to size pixels.
    public static LogoGeometry Design(int size)
    {
        float s = size / 128f;
        return new LogoGeometry
        {
            TileX = 4 * s, TileY = 4 * s, TileW = 120 * s, TileR = 28 * s,
            WinX = 50 * s, WinY = 60 * s, WinW = 58 * s, WinH = 48 * s, WinR = 7 * s, Stroke = 6 * s,
            BallX = 43 * s, BallY = 53 * s, BallR = 17 * s, GapR = 24 * s, RingR = 27 * s,
            CableX = 22.7f * s, CableY = 0, CableW = 5 * s,
            HiX = 37 * s, HiY = 47 * s, HiR = 5 * s,
        };
    }

    // Tray and title bar sizes, snapped so the outline lands on whole pixels.
    public static LogoGeometry Tuned(int size)
    {
        switch (size)
        {
            case 16:
                return new LogoGeometry
                {
                    TileW = 16, TileR = 3.5f,
                    WinX = 6.5f, WinY = 7.5f, WinW = 7, WinH = 6, WinR = 1.5f, Stroke = 1,
                    BallX = 5.5f, BallY = 6.5f, BallR = 2f, GapR = 3.5f, RingR = 4f,
                    CableX = 3f, CableW = 1,
                };
            case 20:
                return new LogoGeometry
                {
                    TileW = 20, TileR = 4.5f,
                    WinX = 7.5f, WinY = 9.5f, WinW = 9, WinH = 7, WinR = 1.5f, Stroke = 1,
                    BallX = 6.75f, BallY = 8.25f, BallR = 2.5f, GapR = 4f, RingR = 4.5f,
                    CableX = 3.58f, CableW = 1,
                    HiX = 5.9f, HiY = 7.4f, HiR = 0.75f,
                };
            case 24:
                return new LogoGeometry
                {
                    TileW = 24, TileR = 5.25f,
                    WinX = 9.5f, WinY = 11.5f, WinW = 11, WinH = 9, WinR = 1.75f, Stroke = 1,
                    BallX = 8.25f, BallY = 10.25f, BallR = 3.25f, GapR = 4.75f, RingR = 5.25f,
                    CableX = 4.31f, CableW = 1,
                    HiX = 7.2f, HiY = 9.2f, HiR = 0.9f,
                };
            case 32:
                return new LogoGeometry
                {
                    TileX = 1, TileY = 1, TileW = 30, TileR = 7,
                    WinX = 13, WinY = 15, WinW = 14, WinH = 12, WinR = 1.75f, Stroke = 2,
                    BallX = 10.75f, BallY = 13.25f, BallR = 4.25f, GapR = 6.25f, RingR = 7.25f,
                    CableX = 5.66f, CableW = 1.5f,
                    HiX = 9.25f, HiY = 11.75f, HiR = 1.25f,
                };
            default:
                return Design(size);
        }
    }
}

public static class LogoRenderer
{
    static readonly Color Yellow = Color.FromArgb(0xF5, 0xC4, 0x00);
    static readonly Color Ink = Color.FromArgb(0x12, 0x12, 0x12);
    static readonly Color Shine = Color.FromArgb(0x4A, 0x4A, 0x4A);

    public static Bitmap Render(LogoGeometry g, int size)
    {
        // Drawn large and box-filtered down, so clip edges are antialiased too.
        int k = (int)Math.Ceiling(1024.0 / size);
        using (var big = new Bitmap(size * k, size * k, PixelFormat.Format32bppPArgb))
        {
            using (var gr = Graphics.FromImage(big))
            {
                gr.SmoothingMode = SmoothingMode.AntiAlias;
                gr.PixelOffsetMode = PixelOffsetMode.HighQuality;
                gr.Clear(Color.Transparent);
                gr.ScaleTransform(k, k);
                Draw(gr, g);
            }
            return Downsample(big, k, size);
        }
    }

    static void Draw(Graphics gr, LogoGeometry g)
    {
        using (var tile = RoundRect(g.TileX, g.TileY, g.TileW, g.TileW, g.TileR))
        using (var brush = new SolidBrush(Yellow))
            gr.FillPath(brush, tile);

        using (var gap = Circle(g.BallX, g.BallY, g.GapR))
        using (var clip = new Region())
        using (var pen = new Pen(Ink, g.Stroke))
        {
            clip.Exclude(gap);
            gr.SetClip(clip, CombineMode.Replace);
            using (var win = RoundRect(g.WinX, g.WinY, g.WinW, g.WinH, g.WinR))
            using (var white = new SolidBrush(Color.White))
            {
                gr.FillPath(white, win);
                gr.DrawPath(pen, win);
            }
            float h = g.Stroke / 2;
            using (var outer = RoundRect(g.WinX - h, g.WinY - h, g.WinW + g.Stroke, g.WinH + g.Stroke, g.WinR + h))
            using (var ring = Circle(g.BallX, g.BallY, g.RingR))
            {
                gr.SetClip(outer, CombineMode.Intersect);
                gr.DrawPath(pen, ring);
            }
            gr.ResetClip();
        }

        using (var pen = new Pen(Ink, g.CableW))
            gr.DrawLine(pen, g.BallX, g.BallY, g.CableX, g.CableY);
        using (var brush = new SolidBrush(Ink))
            gr.FillEllipse(brush, g.BallX - g.BallR, g.BallY - g.BallR, 2 * g.BallR, 2 * g.BallR);
        if (g.HiR > 0)
            using (var brush = new SolidBrush(Shine))
                gr.FillEllipse(brush, g.HiX - g.HiR, g.HiY - g.HiR, 2 * g.HiR, 2 * g.HiR);
    }

    static GraphicsPath RoundRect(float x, float y, float w, float h, float r)
    {
        var p = new GraphicsPath();
        float d = 2 * r;
        p.AddArc(x, y, d, d, 180, 90);
        p.AddArc(x + w - d, y, d, d, 270, 90);
        p.AddArc(x + w - d, y + h - d, d, d, 0, 90);
        p.AddArc(x, y + h - d, d, d, 90, 90);
        p.CloseFigure();
        return p;
    }

    static GraphicsPath Circle(float cx, float cy, float r)
    {
        var p = new GraphicsPath();
        p.AddEllipse(cx - r, cy - r, 2 * r, 2 * r);
        return p;
    }

    // Averages premultiplied pixels, so transparent edges get no dark fringe.
    static Bitmap Downsample(Bitmap big, int k, int size)
    {
        var bd = big.LockBits(new Rectangle(0, 0, big.Width, big.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppPArgb);
        var src = new byte[bd.Stride * big.Height];
        Marshal.Copy(bd.Scan0, src, 0, src.Length);
        int stride = bd.Stride;
        big.UnlockBits(bd);

        var result = new Bitmap(size, size, PixelFormat.Format32bppArgb);
        var od = result.LockBits(new Rectangle(0, 0, size, size), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
        var dst = new byte[od.Stride * size];
        long n = (long)k * k;
        for (int y = 0; y < size; y++)
        {
            for (int x = 0; x < size; x++)
            {
                long b = 0, gr = 0, r = 0, a = 0;
                for (int yy = 0; yy < k; yy++)
                {
                    int row = (y * k + yy) * stride + x * k * 4;
                    for (int xx = 0; xx < k; xx++)
                    {
                        int i = row + xx * 4;
                        b += src[i]; gr += src[i + 1]; r += src[i + 2]; a += src[i + 3];
                    }
                }
                if (a == 0) continue;
                int o = y * od.Stride + x * 4;
                dst[o] = (byte)Math.Min(255, (b * 255 + a / 2) / a);
                dst[o + 1] = (byte)Math.Min(255, (gr * 255 + a / 2) / a);
                dst[o + 2] = (byte)Math.Min(255, (r * 255 + a / 2) / a);
                dst[o + 3] = (byte)((a + n / 2) / n);
            }
        }
        Marshal.Copy(dst, 0, od.Scan0, dst.Length);
        result.UnlockBits(od);
        return result;
    }

    public static byte[] Png(Bitmap bmp)
    {
        using (var ms = new MemoryStream())
        {
            bmp.Save(ms, ImageFormat.Png);
            return ms.ToArray();
        }
    }

    public static void WriteIco(string path, int[] sizes, byte[][] pngs)
    {
        using (var w = new BinaryWriter(File.Create(path)))
        {
            w.Write((ushort)0);
            w.Write((ushort)1);
            w.Write((ushort)sizes.Length);
            int offset = 6 + 16 * sizes.Length;
            for (int i = 0; i < sizes.Length; i++)
            {
                byte dim = (byte)(sizes[i] >= 256 ? 0 : sizes[i]);
                w.Write(dim);
                w.Write(dim);
                w.Write((byte)0);
                w.Write((byte)0);
                w.Write((ushort)1);
                w.Write((ushort)32);
                w.Write((uint)pngs[i].Length);
                w.Write((uint)offset);
                offset += pngs[i].Length;
            }
            foreach (var png in pngs)
                w.Write(png);
        }
    }
}
'@

$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = foreach ($size in $sizes) {
    $bmp = [LogoRenderer]::Render([LogoGeometry]::Tuned($size), $size)
    , [LogoRenderer]::Png($bmp)
    $bmp.Dispose()
}
[LogoRenderer]::WriteIco((Join-Path $PSScriptRoot 'wrecktangle.ico'), [int[]]$sizes, [byte[][]]$pngs)
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'wrecktangle-256.png'), $pngs[-1])
