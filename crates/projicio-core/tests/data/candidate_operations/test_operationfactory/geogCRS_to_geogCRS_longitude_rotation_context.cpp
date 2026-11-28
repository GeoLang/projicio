TEST(operation, geogCRS_to_geogCRS_longitude_rotation_context) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("4807"), // NTF(Paris)
        authFactory->createCoordinateReferenceSystem("4275"), // NTF
        ctxt);
    ASSERT_EQ(list.size(), 2U);
    EXPECT_EQ(list[0]->nameStr(), "NTF (Paris) to NTF (1)");
    EXPECT_EQ(list[0]->exportToPROJString(PROJStringFormatter::create().get()),
              "+proj=pipeline +step +proj=axisswap +order=2,1 +step "
              "+proj=unitconvert +xy_in=grad +xy_out=rad +step +inv "
              "+proj=longlat +ellps=clrk80ign +pm=paris +step "
              "+proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap "
              "+order=2,1");
    EXPECT_EQ(list[1]->nameStr(), "NTF (Paris) to NTF (2)");
    EXPECT_EQ(list[1]->exportToPROJString(PROJStringFormatter::create().get()),
              "+proj=pipeline +step +proj=axisswap +order=2,1 +step "
              "+proj=unitconvert +xy_in=grad +xy_out=rad +step +inv "
              "+proj=longlat +ellps=clrk80ign +pm=2.33720833333333 +step "
              "+proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap "
              "+order=2,1");
}

