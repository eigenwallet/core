import { Footer, Layout, Navbar } from "nextra-theme-docs";
import { Head } from "nextra/components";
import { getPageMap } from "nextra/page-map";
import Logo from "../components/Logo";
import "nextra-theme-docs/style.css";

export const metadata = {
  title: "eigenwallet Docs",
  description: "eigenwallet Docs",
  icons: {
    icon: [
      { url: "/favicon.ico", sizes: "any" },
      { url: "/icon.svg", type: "image/svg+xml" },
    ],
    apple: "/apple-touch-icon.png",
  },
  manifest: "/manifest.webmanifest",
};

export default async function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" dir="ltr" suppressHydrationWarning>
      <Head color={{ hue: 38, saturation: 100 }} />
      <body>
        <Layout
          pageMap={await getPageMap()}
          editLink={null}
          feedback={{ content: null }}
          navbar={
            <Navbar
              logo={<Logo />}
              projectLink="https://github.com/eigenwallet/core"
            />
          }
          footer={<Footer />}
        >
          {children}
        </Layout>
      </body>
    </html>
  );
}
