plugins {
    kotlin("multiplatform") version "2.0.0"
}

group = "ai.muonium"
version = "0.1.0"

repositories {
    mavenCentral()
}

kotlin {
    jvm()
    macosArm64()
    macosX64()
    linuxX64()

    sourceSets {
        commonMain {
            dependencies {}
            kotlin.srcDir("src/main/kotlin")
        }
        commonTest {
            dependencies {
                implementation(kotlin("test"))
            }
            kotlin.srcDir("src/test/kotlin")
        }
    }
}
